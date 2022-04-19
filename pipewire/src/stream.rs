// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! Pipewire Stream

use crate::buffer::Buffer;
use crate::{error::Error, Core, IsLoop, MainLoop, Properties, PropertiesRef};
use bitflags::bitflags;
use spa::result::SpaResult;
use std::fmt::Debug;
use std::{
    ffi::{self, CStr, CString},
    mem, os,
    pin::Pin,
    ptr,
};

#[derive(Debug)]
pub enum StreamState {
    Error(String),
    Unconnected,
    Connecting,
    Paused,
    Streaming,
}

impl StreamState {
    pub(crate) fn from_raw(state: pw_sys::pw_stream_state, error: *const os::raw::c_char) -> Self {
        match state {
            pw_sys::pw_stream_state_PW_STREAM_STATE_UNCONNECTED => StreamState::Unconnected,
            pw_sys::pw_stream_state_PW_STREAM_STATE_CONNECTING => StreamState::Connecting,
            pw_sys::pw_stream_state_PW_STREAM_STATE_PAUSED => StreamState::Paused,
            pw_sys::pw_stream_state_PW_STREAM_STATE_STREAMING => StreamState::Streaming,
            _ => {
                let error = if error.is_null() {
                    "".to_string()
                } else {
                    unsafe { ffi::CStr::from_ptr(error).to_string_lossy().to_string() }
                };

                StreamState::Error(error)
            }
        }
    }
}

/// A wrapper around the pipewire stream interface. Streams are a higher
/// level abstraction around nodes in the graph. A stream can be used to send or
/// receive frames of audio of video data by connecting it to another node.
/// `D` is the user data, to allow passing extra context to the callbacks.
pub struct Stream<D> {
    ptr: ptr::NonNull<pw_sys::pw_stream>,
    // objects that need to stay alive while the Stream is
    _alive: KeepAlive<D>,
}

enum KeepAlive<D> {
    // Stream created with Stream::new()
    Normal {
        _core: Core,
    },
    // Stream created with Stream::simple()
    Simple {
        _events: Pin<Box<pw_sys::pw_stream_events>>,
        _data: Box<ListenerLocalCallbacks<D>>,
    },
    // Temporary stream for callbacks
    Temp,
}

impl<D> Stream<D> {
    /// Create a [`Stream`]
    ///
    /// Initialises a new stream with the given `name` and `properties`.
    pub fn new(core: &Core, name: &str, properties: Properties) -> Result<Self, Error> {
        let name = CString::new(name).expect("Invalid byte in stream name");
        let stream =
            unsafe { pw_sys::pw_stream_new(core.as_ptr(), name.as_ptr(), properties.into_raw()) };
        let stream = ptr::NonNull::new(stream).ok_or(Error::CreationFailed)?;

        Ok(Stream {
            ptr: stream,
            _alive: KeepAlive::Normal {
                _core: core.clone(),
            },
        })
    }

    /// Create a [`Stream`] with custom user data, and connect its event.
    ///
    /// Create a stream directly from a [`MainLoop`]. This avoids having to create
    /// a [`crate::Context`] and [`Core`] yourself in cases that don't require anything
    /// special.
    ///
    /// # Panics
    /// Will panic if `name` contains a 0 byte.
    ///
    /// # Example
    /// ```no_run
    /// use pipewire::prelude::*;
    /// use pipewire::properties;
    ///
    /// let mainloop = pipewire::MainLoop::new()?;
    ///
    /// let mut stream = pipewire::stream::Stream::<i32>::with_user_data(
    ///     &mainloop,
    ///     "video-test",
    ///     properties! {
    ///         *pipewire::keys::MEDIA_TYPE => "Video",
    ///         *pipewire::keys::MEDIA_CATEGORY => "Capture",
    ///         *pipewire::keys::MEDIA_ROLE => "Camera",
    ///     },
    ///     42,
    /// )
    /// .state_changed(|old, new| {
    ///     println!("State changed: {:?} -> {:?}", old, new);
    /// })
    /// .process(|_stream, _user_data| {
    ///     println!("On frame");
    /// })
    /// .create()?;
    /// # Ok::<(), pipewire::Error>(())
    /// ```
    pub fn with_user_data<'a>(
        main_loop: &'a MainLoop,
        name: &str,
        properties: Properties,
        user_data: D,
    ) -> SimpleLocalBuilder<'a, D> {
        let name = CString::new(name).expect("Invalid byte in stream name");

        SimpleLocalBuilder::<D> {
            main_loop,
            name,
            properties,
            callbacks: ListenerLocalCallbacks::with_user_data(user_data),
        }
    }

    /// Add a local listener builder
    #[must_use = "Fluent builder API"]
    pub fn add_local_listener_with_user_data(
        &mut self,
        user_data: D,
    ) -> ListenerLocalBuilder<'_, D> {
        ListenerLocalBuilder {
            stream: self,
            callbacks: ListenerLocalCallbacks::with_user_data(user_data),
        }
    }

    /// Connect the stream
    ///
    /// Tries to connect to the node `id` in the given `direction`. If no node
    /// is provided then any suitable node will be used.
    // FIXME: high-level API for params
    pub fn connect(
        &self,
        direction: spa::Direction,
        id: Option<u32>,
        flags: StreamFlags,
        params: &mut [*const spa_sys::spa_pod],
    ) -> Result<(), Error> {
        let r = unsafe {
            pw_sys::pw_stream_connect(
                self.as_ptr(),
                direction.as_raw(),
                id.unwrap_or(crate::constants::ID_ANY),
                flags.bits(),
                params.as_mut_ptr(),
                params.len() as u32,
            )
        };

        SpaResult::from_c(r).into_sync_result()?;
        Ok(())
    }

    /// Update Parameters
    ///
    /// Call from the `param_changed` callback to negotiate a new set of
    /// parameters for the stream.
    // FIXME: high-level API for params
    pub fn update_params(&self, params: &mut [*const spa_sys::spa_pod]) -> Result<(), Error> {
        let r = unsafe {
            pw_sys::pw_stream_update_params(self.as_ptr(), params.as_mut_ptr(), params.len() as u32)
        };

        SpaResult::from_c(r).into_sync_result()?;
        Ok(())
    }

    /// Activate or deactivate the stream
    pub fn set_active(&self, active: bool) -> Result<(), Error> {
        let r = unsafe { pw_sys::pw_stream_set_active(self.as_ptr(), active) };

        SpaResult::from_c(r).into_sync_result()?;
        Ok(())
    }

    /// Take a Buffer from the Stream
    ///
    /// Removes a buffer from the stream. If this is an input stream the buffer
    /// will contain data ready to process. If this is an output stream it can
    /// be filled.
    ///
    /// # Safety
    ///
    /// The pointer returned could be NULL if no buffer is available. The buffer
    /// should be returned to the stream once processing is complete.
    pub unsafe fn dequeue_raw_buffer(&self) -> *mut pw_sys::pw_buffer {
        pw_sys::pw_stream_dequeue_buffer(self.as_ptr())
    }

    pub fn dequeue_buffer(&self) -> Option<Buffer<D>> {
        unsafe { Buffer::from_raw(self.dequeue_raw_buffer(), self) }
    }

    /// Return a Buffer to the Stream
    ///
    /// Give back a buffer once processing is complete. Use this to queue up a
    /// frame for an output stream, or return the buffer to the pool ready to
    /// receive new data for an input stream.
    ///
    /// # Safety
    ///
    /// The buffer pointer should be one obtained from this stream instance by
    /// a call to [Stream::dequeue_raw_buffer()].
    pub unsafe fn queue_raw_buffer(&self, buffer: *mut pw_sys::pw_buffer) {
        pw_sys::pw_stream_queue_buffer(self.as_ptr(), buffer);
    }

    fn as_ptr(&self) -> *mut pw_sys::pw_stream {
        self.ptr.as_ptr()
    }

    /// Disconnect the stream
    pub fn disconnect(&self) -> Result<(), Error> {
        let r = unsafe { pw_sys::pw_stream_disconnect(self.as_ptr()) };

        SpaResult::from_c(r).into_sync_result()?;
        Ok(())
    }

    /// Set the stream in error state
    ///
    /// # Panics
    /// Will panic if `error` contains a 0 byte.
    ///
    pub fn set_error(&mut self, res: i32, error: &str) {
        let error = CString::new(error).expect("failed to convert error to CString");
        unsafe {
            pw_sys::pw_stream_set_error(self.as_ptr(), res, error.as_c_str().as_ptr());
        }
    }

    /// Flush the stream. When  `drain` is `true`, the `drained` callback will
    /// be called when all data is played or recorded.
    pub fn flush(&self, drain: bool) -> Result<(), Error> {
        let r = unsafe { pw_sys::pw_stream_flush(self.as_ptr(), drain) };

        SpaResult::from_c(r).into_sync_result()?;
        Ok(())
    }

    // TODO: pw_stream_set_control()

    // getters

    /// Get the name of the stream.
    pub fn name(&self) -> String {
        let name = unsafe {
            let name = pw_sys::pw_stream_get_name(self.as_ptr());
            CStr::from_ptr(name)
        };

        name.to_string_lossy().to_string()
    }

    /// Get the current state of the stream.
    pub fn state(&self) -> StreamState {
        let mut error: *const std::os::raw::c_char = ptr::null();
        let state =
            unsafe { pw_sys::pw_stream_get_state(self.as_ptr(), (&mut error) as *mut *const _) };
        StreamState::from_raw(state, error)
    }

    /// Get the properties of the stream.
    pub fn properties(&self) -> PropertiesRef<'_> {
        unsafe {
            let props = pw_sys::pw_stream_get_properties(self.as_ptr());
            let props = ptr::NonNull::new(props as *mut _).expect("stream properties is NULL");
            PropertiesRef::from_ptr(props)
        }
    }

    /// Get the node ID of the stream.
    pub fn node_id(&self) -> u32 {
        unsafe { pw_sys::pw_stream_get_node_id(self.as_ptr()) }
    }

    // TODO: pw_stream_get_core()
    // TODO: pw_stream_get_time()
}

impl<D: Default> Stream<D> {
    /// Create a [`Stream`] and connect its event.
    ///
    /// Create a stream directly from a [`MainLoop`]. This avoids having to create
    /// a [`crate::Context`] and [`Core`] yourself in cases that don't require anything
    /// special.
    ///
    /// # Panics
    /// Will panic if `name` contains a 0 byte.
    ///
    /// # Example
    /// ```no_run
    /// use pipewire::prelude::*;
    /// use pipewire::properties;
    ///
    /// let mainloop = pipewire::MainLoop::new()?;
    ///
    /// let mut stream = pipewire::stream::Stream::<()>::simple(
    ///     &mainloop,
    ///     "video-test",
    ///     properties! {
    ///         *pipewire::keys::MEDIA_TYPE => "Video",
    ///         *pipewire::keys::MEDIA_CATEGORY => "Capture",
    ///         *pipewire::keys::MEDIA_ROLE => "Camera",
    ///     },
    /// )
    /// .state_changed(|old, new| {
    ///     println!("State changed: {:?} -> {:?}", old, new);
    /// })
    /// .process(|_stream, _user_data| {
    ///     println!("On frame");
    /// })
    /// .create()?;
    /// # Ok::<(), pipewire::Error>(())
    /// ```
    #[must_use]
    pub fn simple<'a>(
        main_loop: &'a MainLoop,
        name: &str,
        properties: Properties,
    ) -> SimpleLocalBuilder<'a, D> {
        Self::with_user_data(main_loop, name, properties, Default::default())
    }

    /// Add a local listener builder
    #[must_use = "Fluent builder API"]
    pub fn add_local_listener(&mut self) -> ListenerLocalBuilder<'_, D> {
        ListenerLocalBuilder {
            stream: self,
            callbacks: ListenerLocalCallbacks::with_user_data(Default::default()),
        }
    }
}

impl<D> std::fmt::Debug for Stream<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stream")
            .field("name", &self.name())
            .field("state", &self.state())
            .field("node-id", &self.node_id())
            .field("properties", &self.properties())
            .finish()
    }
}

type ParamChangedCB<D> = dyn Fn(u32, &mut D, *const spa_sys::spa_pod);
type ProcessCB<D> = dyn Fn(&Stream<D>, &mut D);

pub struct ListenerLocalCallbacks<D> {
    pub state_changed: Option<Box<dyn Fn(StreamState, StreamState)>>,
    pub control_info: Option<Box<dyn Fn(u32, *const pw_sys::pw_stream_control)>>,
    #[allow(clippy::type_complexity)]
    pub io_changed: Option<Box<dyn Fn(u32, *mut os::raw::c_void, u32)>>,
    pub param_changed: Option<Box<ParamChangedCB<D>>>,
    pub add_buffer: Option<Box<dyn Fn(*mut pw_sys::pw_buffer)>>,
    pub remove_buffer: Option<Box<dyn Fn(*mut pw_sys::pw_buffer)>>,
    pub process: Option<Box<ProcessCB<D>>>,
    pub drained: Option<Box<dyn Fn()>>,
    pub user_data: D,
    stream: Option<ptr::NonNull<pw_sys::pw_stream>>,
}

impl<D> ListenerLocalCallbacks<D> {
    fn with_user_data(user_data: D) -> Self {
        ListenerLocalCallbacks {
            process: Default::default(),
            stream: Default::default(),
            drained: Default::default(),
            add_buffer: Default::default(),
            control_info: Default::default(),
            io_changed: Default::default(),
            param_changed: Default::default(),
            remove_buffer: Default::default(),
            state_changed: Default::default(),
            user_data,
        }
    }

    pub(crate) fn into_raw(
        self,
    ) -> (
        Pin<Box<pw_sys::pw_stream_events>>,
        Box<ListenerLocalCallbacks<D>>,
    ) {
        let callbacks = Box::new(self);

        unsafe extern "C" fn on_state_changed<D>(
            data: *mut os::raw::c_void,
            old: pw_sys::pw_stream_state,
            new: pw_sys::pw_stream_state,
            error: *const os::raw::c_char,
        ) {
            if let Some(state) = (data as *mut ListenerLocalCallbacks<D>).as_ref() {
                if let Some(ref cb) = state.state_changed {
                    let old = StreamState::from_raw(old, error);
                    let new = StreamState::from_raw(new, error);
                    cb(old, new)
                };
            }
        }

        unsafe extern "C" fn on_control_info<D>(
            data: *mut os::raw::c_void,
            id: u32,
            control: *const pw_sys::pw_stream_control,
        ) {
            if let Some(state) = (data as *mut ListenerLocalCallbacks<D>).as_ref() {
                if let Some(ref cb) = state.control_info {
                    cb(id, control);
                }
            }
        }

        unsafe extern "C" fn on_io_changed<D>(
            data: *mut os::raw::c_void,
            id: u32,
            area: *mut os::raw::c_void,
            size: u32,
        ) {
            if let Some(state) = (data as *mut ListenerLocalCallbacks<D>).as_ref() {
                if let Some(ref cb) = state.io_changed {
                    cb(id, area, size);
                }
            }
        }

        unsafe extern "C" fn on_param_changed<D>(
            data: *mut os::raw::c_void,
            id: u32,
            param: *const spa_sys::spa_pod,
        ) {
            if let Some(state) = (data as *mut ListenerLocalCallbacks<D>).as_mut() {
                if let Some(ref cb) = state.param_changed {
                    cb(id, &mut state.user_data, param);
                }
            }
        }

        unsafe extern "C" fn on_add_buffer<D>(
            data: *mut ::std::os::raw::c_void,
            buffer: *mut pw_sys::pw_buffer,
        ) {
            if let Some(state) = (data as *mut ListenerLocalCallbacks<D>).as_ref() {
                if let Some(ref cb) = state.add_buffer {
                    cb(buffer);
                }
            }
        }

        unsafe extern "C" fn on_remove_buffer<D>(
            data: *mut ::std::os::raw::c_void,
            buffer: *mut pw_sys::pw_buffer,
        ) {
            if let Some(state) = (data as *mut ListenerLocalCallbacks<D>).as_ref() {
                if let Some(ref cb) = state.remove_buffer {
                    cb(buffer);
                }
            }
        }

        unsafe extern "C" fn on_process<D>(data: *mut ::std::os::raw::c_void) {
            if let Some(state) = (data as *mut ListenerLocalCallbacks<D>).as_mut() {
                if let Some(ref cb) = state.process {
                    let stream = state
                        .stream
                        .map(|ptr| Stream {
                            ptr,
                            _alive: KeepAlive::Temp,
                        })
                        .expect("stream cannot be null");
                    cb(&stream, &mut state.user_data);
                }
            }
        }

        unsafe extern "C" fn on_drained<D>(data: *mut ::std::os::raw::c_void) {
            if let Some(state) = (data as *mut ListenerLocalCallbacks<D>).as_ref() {
                if let Some(ref cb) = state.drained {
                    cb();
                }
            }
        }

        let events = unsafe {
            let mut events: Pin<Box<pw_sys::pw_stream_events>> = Box::pin(mem::zeroed());
            events.version = pw_sys::PW_VERSION_STREAM_EVENTS;

            if callbacks.state_changed.is_some() {
                events.state_changed = Some(on_state_changed::<D>);
            }
            if callbacks.control_info.is_some() {
                events.control_info = Some(on_control_info::<D>);
            }
            if callbacks.io_changed.is_some() {
                events.io_changed = Some(on_io_changed::<D>);
            }
            if callbacks.param_changed.is_some() {
                events.param_changed = Some(on_param_changed::<D>);
            }
            if callbacks.add_buffer.is_some() {
                events.add_buffer = Some(on_add_buffer::<D>);
            }
            if callbacks.remove_buffer.is_some() {
                events.remove_buffer = Some(on_remove_buffer::<D>);
            }
            if callbacks.process.is_some() {
                events.process = Some(on_process::<D>);
            }
            if callbacks.drained.is_some() {
                events.drained = Some(on_drained::<D>);
            }

            events
        };

        (events, callbacks)
    }
}

pub trait ListenerBuilderT<D>: Sized {
    fn callbacks(&mut self) -> &mut ListenerLocalCallbacks<D>;

    /// Set the callback for the `state_changed` event.
    fn state_changed<F>(mut self, callback: F) -> Self
    where
        F: Fn(StreamState, StreamState) + 'static,
    {
        self.callbacks().state_changed = Some(Box::new(callback));
        self
    }

    /// Set the callback for the `control_info` event.
    fn control_info<F>(mut self, callback: F) -> Self
    where
        F: Fn(u32, *const pw_sys::pw_stream_control) + 'static,
    {
        self.callbacks().control_info = Some(Box::new(callback));
        self
    }

    /// Set the callback for the `io_changed` event.
    fn io_changed<F>(mut self, callback: F) -> Self
    where
        F: Fn(u32, *mut os::raw::c_void, u32) + 'static,
    {
        self.callbacks().io_changed = Some(Box::new(callback));
        self
    }

    /// Set the callback for the `param_changed` event.
    fn param_changed<F>(mut self, callback: F) -> Self
    where
        F: Fn(u32, &mut D, *const spa_sys::spa_pod) + 'static,
    {
        self.callbacks().param_changed = Some(Box::new(callback));
        self
    }

    /// Set the callback for the `add_buffer` event.
    fn add_buffer<F>(mut self, callback: F) -> Self
    where
        F: Fn(*mut pw_sys::pw_buffer) + 'static,
    {
        self.callbacks().add_buffer = Some(Box::new(callback));
        self
    }

    /// Set the callback for the `remove_buffer` event.
    fn remove_buffer<F>(mut self, callback: F) -> Self
    where
        F: Fn(*mut pw_sys::pw_buffer) + 'static,
    {
        self.callbacks().remove_buffer = Some(Box::new(callback));
        self
    }

    /// Set the callback for the `process` event.
    fn process<F>(mut self, callback: F) -> Self
    where
        F: Fn(&Stream<D>, &mut D) + 'static,
    {
        self.callbacks().process = Some(Box::new(callback));
        self
    }

    /// Set the callback for the `drained` event.
    fn drained<F>(mut self, callback: F) -> Self
    where
        F: Fn() + 'static,
    {
        self.callbacks().drained = Some(Box::new(callback));
        self
    }
}

pub struct ListenerLocalBuilder<'a, D> {
    stream: &'a mut Stream<D>,
    callbacks: ListenerLocalCallbacks<D>,
}

impl<'a, D: Default> ListenerBuilderT<D> for ListenerLocalBuilder<'a, D> {
    fn callbacks(&mut self) -> &mut ListenerLocalCallbacks<D> {
        &mut self.callbacks
    }
}

impl<'a, D> ListenerLocalBuilder<'a, D> {
    //// Register the Callbacks
    ///
    /// Stop building the listener and register it on the stream. Returns a
    /// `StreamListener` handlle that will un-register the listener on drop.
    pub fn register(self) -> Result<StreamListener<D>, Error> {
        let (events, data) = self.callbacks.into_raw();
        let (listener, data) = unsafe {
            let listener: Box<spa_sys::spa_hook> = Box::new(mem::zeroed());
            let raw_listener = Box::into_raw(listener);
            let raw_data = Box::into_raw(data);
            pw_sys::pw_stream_add_listener(
                self.stream.as_ptr(),
                raw_listener,
                events.as_ref().get_ref(),
                raw_data as *mut _,
            );
            (Box::from_raw(raw_listener), Box::from_raw(raw_data))
        };
        Ok(StreamListener {
            listener,
            _events: events,
            _data: data,
        })
    }
}

pub struct SimpleLocalBuilder<'a, D> {
    main_loop: &'a MainLoop,
    name: CString,
    properties: Properties,
    callbacks: ListenerLocalCallbacks<D>,
}

impl<'a, D> ListenerBuilderT<D> for SimpleLocalBuilder<'a, D> {
    fn callbacks(&mut self) -> &mut ListenerLocalCallbacks<D> {
        &mut self.callbacks
    }
}

impl<'a, D> SimpleLocalBuilder<'a, D> {
    pub fn create(self) -> Result<Stream<D>, Error> {
        let (events, data) = self.callbacks.into_raw();
        let data = Box::into_raw(data);
        let (stream, mut data) = unsafe {
            let stream = pw_sys::pw_stream_new_simple(
                self.main_loop.as_loop().as_ptr(),
                self.name.as_ptr(),
                self.properties.into_raw(),
                events.as_ref().get_ref(),
                data as *mut _,
            );
            (stream, Box::from_raw(data))
        };
        let stream = ptr::NonNull::new(stream).ok_or(Error::CreationFailed)?;
        data.stream = Some(stream);

        // pw_stream does not keep a pointer on the loop so no need to ensure it stays alive
        Ok(Stream {
            ptr: stream,
            _alive: KeepAlive::Simple {
                _events: events,
                _data: data,
            },
        })
    }
}

pub struct StreamListener<D> {
    listener: Box<spa_sys::spa_hook>,
    // Need to stay allocated while the listener is registered
    _events: Pin<Box<pw_sys::pw_stream_events>>,
    _data: Box<ListenerLocalCallbacks<D>>,
}

impl<D> StreamListener<D> {
    /// Stop the listener from receiving any events
    ///
    /// Removes the listener registration and cleans up allocated ressources.
    pub fn unregister(self) {
        // do nothing, drop will clean up.
    }
}

impl<D> std::ops::Drop for StreamListener<D> {
    fn drop(&mut self) {
        spa::hook::remove(*self.listener);
    }
}

bitflags! {
    /// Extra flags that can be used in [`Stream::connect()`]
    pub struct StreamFlags: pw_sys::pw_stream_flags {
        const AUTOCONNECT = pw_sys::pw_stream_flags_PW_STREAM_FLAG_AUTOCONNECT;
        const INACTIVE = pw_sys::pw_stream_flags_PW_STREAM_FLAG_INACTIVE;
        const MAP_BUFFERS = pw_sys::pw_stream_flags_PW_STREAM_FLAG_MAP_BUFFERS;
        const DRIVER = pw_sys::pw_stream_flags_PW_STREAM_FLAG_DRIVER;
        const RT_PROCESS = pw_sys::pw_stream_flags_PW_STREAM_FLAG_RT_PROCESS;
        const NO_CONVERT = pw_sys::pw_stream_flags_PW_STREAM_FLAG_NO_CONVERT;
        const EXCLUSIVE = pw_sys::pw_stream_flags_PW_STREAM_FLAG_EXCLUSIVE;
        const DONT_RECONNECT = pw_sys::pw_stream_flags_PW_STREAM_FLAG_DONT_RECONNECT;
        const ALLOC_BUFFERS = pw_sys::pw_stream_flags_PW_STREAM_FLAG_ALLOC_BUFFERS;
    }
}
