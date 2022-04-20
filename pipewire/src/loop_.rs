// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

use std::{convert::TryInto, ops::Deref, os::unix::prelude::*, ptr, time::Duration};

use libc::{c_int, c_void};
use signal::Signal;
use spa::{flags::IoFlags, result::SpaResult, spa_interface_call_method};

use crate::utils::assert_main_thread;

/// A transparent wrapper around a raw [`pw_loop`](`pw_sys::pw_loop`).
/// It is usually only seen in a reference (`&LoopRef`).
///
/// This type is similar to rusts [`str`] type,
/// where [`&str`](`std::str`) is a reference to a slice of characters,
/// and [`&LoopRef`](`LoopRef`) is a reference to a [`pw_loop`](`pw_sys::pw_loop`).
///
/// Like with [`str`] and [`String`], an owned version, [`Loop`], is available,
/// which can create of [`pw_loop`](`pw_sys::pw_loop`) and lets you own them,
/// but other objects, such as [`MainLoop`](`crate::MainLoop`), also contain them.
#[repr(transparent)]
pub struct LoopRef(pw_sys::pw_loop);

impl LoopRef {
    pub fn as_raw(&self) -> &pw_sys::pw_loop {
        &self.0
    }

    pub fn as_ptr(&self) -> *mut pw_sys::pw_loop {
        &self.0 as *const _ as *mut _
    }

    /// Get the file descriptor backing this loop.
    pub fn fd(&self) -> RawFd {
        unsafe {
            let mut iface = self.as_raw().control.as_ref().unwrap().iface;

            spa_interface_call_method!(
                &mut iface as *mut spa_sys::spa_interface,
                spa_sys::spa_loop_control_methods,
                get_fd,
            )
        }
    }

    /// Enter a loop
    ///
    /// Start an iteration of the loop. This function should be called
    /// before calling iterate and is typically used to capture the thread
    /// that this loop will run in.
    pub fn enter(&self) {
        unsafe {
            let mut iface = self.as_raw().control.as_ref().unwrap().iface;

            spa_interface_call_method!(
                &mut iface as *mut spa_sys::spa_interface,
                spa_sys::spa_loop_control_methods,
                enter,
            )
        }
    }

    /// Leave a loop
    ///
    /// Ends the iteration of a loop. This should be called after calling
    /// iterate.
    pub fn leave(&self) {
        unsafe {
            let mut iface = self.as_raw().control.as_ref().unwrap().iface;

            spa_interface_call_method!(
                &mut iface as *mut spa_sys::spa_interface,
                spa_sys::spa_loop_control_methods,
                leave,
            )
        }
    }

    /// Perform one iteration of the loop.
    ///
    /// An optional timeout can be provided.
    /// 0 for no timeout, -1 for infinite timeout.
    ///
    /// This function will block
    /// up to the provided timeout and then dispatch the fds with activity.
    /// The number of dispatched fds is returned.
    ///
    /// Before calling this, you should call [`Self::enter()`] on the loop, and [`Self::leave()`] afterwards.
    ///
    /// # Panics
    /// This function will panic if the provided timeout as milliseconds does not fit inside a
    /// `c_int` integer.
    pub fn iterate(&self, timeout: std::time::Duration) -> i32 {
        unsafe {
            let mut iface = self.as_raw().control.as_ref().unwrap().iface;

            let timeout: c_int = timeout
                .as_millis()
                .try_into()
                .expect("Provided timeout does not fit in a c_int");

            spa_interface_call_method!(
                &mut iface as *mut spa_sys::spa_interface,
                spa_sys::spa_loop_control_methods,
                iterate,
                timeout
            )
        }
    }

    /// Register some type of IO object with a callback that is called when reading/writing on the IO object
    /// is available.
    ///
    /// The specified `event_mask` determines whether to trigger when either input, output, or any of the two is avaiable.
    ///
    /// The returned IoSource needs to take ownership of the IO object, but will provide a reference to the callback when called.
    #[must_use]
    pub fn add_io<I, F>(&self, io: I, event_mask: IoFlags, callback: F) -> IoSource<I>
    where
        I: AsRawFd,
        F: Fn(&mut I) + 'static,
        Self: Sized,
    {
        unsafe extern "C" fn call_closure<I>(data: *mut c_void, _fd: RawFd, _mask: u32)
        where
            I: AsRawFd,
        {
            let (io, callback) = (data as *mut IoSourceData<I>).as_mut().unwrap();
            callback(io);
        }

        let fd = io.as_raw_fd();
        let data = Box::into_raw(Box::new((io, Box::new(callback) as Box<dyn Fn(&mut I)>)));

        let (source, data) = unsafe {
            let mut iface = self.as_raw().utils.as_ref().unwrap().iface;

            let source = spa_interface_call_method!(
                &mut iface as *mut spa_sys::spa_interface,
                spa_sys::spa_loop_utils_methods,
                add_io,
                fd,
                // FIXME: User provided mask instead
                event_mask.bits(),
                // Never let the loop close the fd, this should be handled via `Drop` implementations.
                false,
                Some(call_closure::<I>),
                data as *mut _
            );

            (source, Box::from_raw(data))
        };

        let ptr = ptr::NonNull::new(source).expect("source is NULL");

        IoSource {
            ptr,
            loop_: self,
            _data: data,
        }
    }

    /// Register a callback to be called whenever the loop is idle.
    ///
    /// This can be enabled and disabled as needed with the `enabled` parameter,
    /// and also with the `enable` method on the returned source.
    #[must_use]
    pub fn add_idle<F>(&self, enabled: bool, callback: F) -> IdleSource
    where
        F: Fn() + 'static,
    {
        unsafe extern "C" fn call_closure<F>(data: *mut c_void)
        where
            F: Fn(),
        {
            let callback = (data as *mut F).as_ref().unwrap();
            callback();
        }

        let data = Box::into_raw(Box::new(callback));

        let (source, data) = unsafe {
            let mut iface = self.as_raw().utils.as_ref().unwrap().iface;

            let source = spa_interface_call_method!(
                &mut iface as *mut spa_sys::spa_interface,
                spa_sys::spa_loop_utils_methods,
                add_idle,
                enabled,
                Some(call_closure::<F>),
                data as *mut _
            );

            (source, Box::from_raw(data))
        };

        let ptr = ptr::NonNull::new(source).expect("source is NULL");

        IdleSource {
            ptr,
            loop_: self,
            _data: data,
        }
    }

    /// Register a signal with a callback that is called when the signal is sent.
    ///
    /// For example, this can be used to quit the loop when the process receives the `SIGTERM` signal.
    #[must_use]
    pub fn add_signal_local<F>(&self, signal: Signal, callback: F) -> SignalSource
    where
        F: Fn() + 'static,
        Self: Sized,
    {
        assert_main_thread();

        unsafe extern "C" fn call_closure<F>(data: *mut c_void, _signal: c_int)
        where
            F: Fn(),
        {
            let callback = (data as *mut F).as_ref().unwrap();
            callback();
        }

        let data = Box::into_raw(Box::new(callback));

        let (source, data) = unsafe {
            let mut iface = self.as_raw().utils.as_ref().unwrap().iface;

            let source = spa_interface_call_method!(
                &mut iface as *mut spa_sys::spa_interface,
                spa_sys::spa_loop_utils_methods,
                add_signal,
                signal as c_int,
                Some(call_closure::<F>),
                data as *mut _
            );

            (source, Box::from_raw(data))
        };

        let ptr = ptr::NonNull::new(source).expect("source is NULL");

        SignalSource {
            ptr,
            loop_: self,
            _data: data,
        }
    }

    /// Register a new event with a callback that is called when the event happens.
    ///
    /// The returned [`EventSource`] can be used to trigger the event.
    #[must_use]
    pub fn add_event<F>(&self, callback: F) -> EventSource
    where
        F: Fn() + 'static,
        Self: Sized,
    {
        unsafe extern "C" fn call_closure<F>(data: *mut c_void, _count: u64)
        where
            F: Fn(),
        {
            let callback = (data as *mut F).as_ref().unwrap();
            callback();
        }

        let data = Box::into_raw(Box::new(callback));

        let (source, data) = unsafe {
            let mut iface = self.as_raw().utils.as_ref().unwrap().iface;

            let source = spa_interface_call_method!(
                &mut iface as *mut spa_sys::spa_interface,
                spa_sys::spa_loop_utils_methods,
                add_event,
                Some(call_closure::<F>),
                data as *mut _
            );
            (source, Box::from_raw(data))
        };

        let ptr = ptr::NonNull::new(source).expect("source is NULL");

        EventSource {
            ptr,
            loop_: self,
            _data: data,
        }
    }

    /// Register a timer with the loop with a callback that is called after the timer expired.
    ///
    /// The timer will start out inactive, and the returned [`TimerSource`] can be used to arm the timer, or disarm it again.
    ///
    /// The callback will be provided with the number of timer expirations since the callback was last called.
    #[must_use]
    pub fn add_timer<F>(&self, callback: F) -> TimerSource
    where
        F: Fn(u64) + 'static,
        Self: Sized,
    {
        unsafe extern "C" fn call_closure<F>(data: *mut c_void, expirations: u64)
        where
            F: Fn(u64),
        {
            let callback = (data as *mut F).as_ref().unwrap();
            callback(expirations);
        }

        let data = Box::into_raw(Box::new(callback));

        let (source, data) = unsafe {
            let mut iface = self.as_raw().utils.as_ref().unwrap().iface;

            let source = spa_interface_call_method!(
                &mut iface as *mut spa_sys::spa_interface,
                spa_sys::spa_loop_utils_methods,
                add_timer,
                Some(call_closure::<F>),
                data as *mut _
            );
            (source, Box::from_raw(data))
        };

        let ptr = ptr::NonNull::new(source).expect("source is NULL");

        TimerSource {
            ptr,
            loop_: self,
            _data: data,
        }
    }

    /// Destroy a source that belongs to this loop.
    ///
    /// # Safety
    /// The provided source must belong to this loop.
    unsafe fn destroy_source<S>(&self, source: &S)
    where
        S: IsSource,
        Self: Sized,
    {
        let mut iface = self.as_raw().utils.as_ref().unwrap().iface;

        spa_interface_call_method!(
            &mut iface as *mut spa_sys::spa_interface,
            spa_sys::spa_loop_utils_methods,
            destroy_source,
            source.as_ptr()
        )
    }
}

pub struct Loop {
    ptr: *mut pw_sys::pw_loop,
}

impl Loop {
    /// Create a new loop from a raw [`pw_loop`](`pw_sys::pw_loop`), taking ownership of it.
    ///
    /// # Safety
    /// The provided pointer must point to a valid, well aligned [`pw_loop`](`pw_sys::pw_loop`).
    ///
    /// The raw loop should not be manually destroyed or moved, as the new [`Loop`] takes ownership of it.
    pub unsafe fn from_raw(ptr: *mut pw_sys::pw_loop) -> Self {
        Loop { ptr }
    }

    /// Create a new [`Loop`].
    pub fn new() -> Self {
        // This is a potential "entry point" to the library, so we need to ensure it is initialized.
        crate::init();

        unsafe { Self::from_raw(pw_sys::pw_loop_new(std::ptr::null())) }
    }

    // TODO: fn with_props

    /// Consume the [`Loop`] and returns the underlying raw [`pw_loop`](`pw_sys::pw_loop`).
    ///
    /// After calling this function, the caller has ownership of the raw [`pw_loop`](`pw_sys::pw_loop`),
    /// and should ensure it is properly destroyed when not used any longer.
    pub fn into_raw(self) -> *mut pw_sys::pw_loop {
        std::mem::ManuallyDrop::new(self).ptr
    }
}

impl IsLoop for Loop {
    fn as_loop(&self) -> &LoopRef {
        self.as_ref()
    }
}

impl std::convert::AsRef<LoopRef> for Loop {
    fn as_ref(&self) -> &LoopRef {
        self.deref()
    }
}

impl std::ops::Deref for Loop {
    type Target = LoopRef;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.ptr as *mut LoopRef) }
    }
}

impl std::ops::Drop for Loop {
    fn drop(&mut self) {
        unsafe { pw_sys::pw_loop_destroy(self.ptr) }
    }
}

/// A trait that allows conversion of any kind of pipewire loop to its underlying `pw_loop`.
///
/// Different kinds of events, such as receiving a signal (e.g. SIGTERM) can be attached to that underlying loop.
pub trait IsLoop {
    fn as_loop(&self) -> &LoopRef;
}

pub trait IsSource {
    /// Return a valid pointer to a raw `spa_source`.
    fn as_ptr(&self) -> *mut spa_sys::spa_source;
}

type IoSourceData<I> = (I, Box<dyn Fn(&mut I) + 'static>);

/// A source that can be used to react to IO events.
///
/// This source can be obtained by calling [`add_io`](`LoopRef::add_io`) on a loop, registering a callback to it.
pub struct IoSource<'l, I>
where
    I: AsRawFd,
{
    ptr: ptr::NonNull<spa_sys::spa_source>,
    loop_: &'l LoopRef,
    // Store data wrapper to prevent leak
    _data: Box<IoSourceData<I>>,
}

impl<'l, I> IsSource for IoSource<'l, I>
where
    I: AsRawFd,
{
    fn as_ptr(&self) -> *mut spa_sys::spa_source {
        self.ptr.as_ptr()
    }
}

impl<'l, I> Drop for IoSource<'l, I>
where
    I: AsRawFd,
{
    fn drop(&mut self) {
        unsafe { self.loop_.destroy_source(self) }
    }
}

/// A source that can be used to have a callback called when the loop is idle.
///
/// This source can be obtained by calling [`add_idle`](`LoopRef::add_idle`) on a loop, registering a callback to it.
pub struct IdleSource<'l> {
    ptr: ptr::NonNull<spa_sys::spa_source>,
    loop_: &'l LoopRef,
    // Store data wrapper to prevent leak
    _data: Box<dyn Fn() + 'static>,
}

impl<'l> IdleSource<'l> {
    /// Set the source as enabled or disabled, allowing or preventing the callback from being called.
    pub fn enable(&self, enable: bool) {
        unsafe {
            let mut iface = self.loop_.as_raw().utils.as_ref().unwrap().iface;

            spa_interface_call_method!(
                &mut iface as *mut spa_sys::spa_interface,
                spa_sys::spa_loop_utils_methods,
                enable_idle,
                self.as_ptr(),
                enable
            );
        }
    }
}

impl<'l> IsSource for IdleSource<'l> {
    fn as_ptr(&self) -> *mut spa_sys::spa_source {
        self.ptr.as_ptr()
    }
}

impl<'l> Drop for IdleSource<'l> {
    fn drop(&mut self) {
        unsafe { self.loop_.destroy_source(self) }
    }
}

/// A source that can be used to react to signals.
///
/// This source can be obtained by calling [`add_signal_local`](`LoopRef::add_signal_local`) on a loop, registering a callback to it.
pub struct SignalSource<'l> {
    ptr: ptr::NonNull<spa_sys::spa_source>,
    loop_: &'l LoopRef,
    // Store data wrapper to prevent leak
    _data: Box<dyn Fn() + 'static>,
}

impl<'l> IsSource for SignalSource<'l> {
    fn as_ptr(&self) -> *mut spa_sys::spa_source {
        self.ptr.as_ptr()
    }
}

impl<'l> Drop for SignalSource<'l> {
    fn drop(&mut self) {
        unsafe { self.loop_.destroy_source(self) }
    }
}

/// A source that can be used to signal to a loop that an event has occurred.
///
/// This source can be obtained by calling [`add_event`](`LoopRef::add_event`) on a loop, registering a callback to it.
///
/// By calling [`signal`](`EventSource::signal`) on the `EventSource`, the loop is signaled that the event has occurred.
/// It will then call the callback at the next possible occasion.
pub struct EventSource<'l> {
    ptr: ptr::NonNull<spa_sys::spa_source>,
    loop_: &'l LoopRef,
    // Store data wrapper to prevent leak
    _data: Box<dyn Fn() + 'static>,
}

impl<'l> IsSource for EventSource<'l> {
    fn as_ptr(&self) -> *mut spa_sys::spa_source {
        self.ptr.as_ptr()
    }
}

impl<'l> EventSource<'l> {
    /// Signal the loop associated with this source that the event has occurred,
    /// to make the loop call the callback at the next possible occasion.
    pub fn signal(&self) -> SpaResult {
        let res = unsafe {
            let mut iface = self.loop_.as_raw().utils.as_ref().unwrap().iface;

            spa_interface_call_method!(
                &mut iface as *mut spa_sys::spa_interface,
                spa_sys::spa_loop_utils_methods,
                signal_event,
                self.as_ptr()
            )
        };

        SpaResult::from_c(res)
    }
}

impl<'l> Drop for EventSource<'l> {
    fn drop(&mut self) {
        unsafe { self.loop_.destroy_source(self) }
    }
}

/// A source that can be used to have a callback called on a timer.
///
/// This source can be obtained by calling [`add_timer`](`LoopRef::add_timer`) on a loop, registering a callback to it.
///
/// The timer starts out inactive.
/// You can arm or disarm the timer by calling [`update_timer`](`Self::update_timer`).
pub struct TimerSource<'l> {
    ptr: ptr::NonNull<spa_sys::spa_source>,
    loop_: &'l LoopRef,
    // Store data wrapper to prevent leak
    _data: Box<dyn Fn(u64) + 'static>,
}

impl<'l> TimerSource<'l> {
    /// Arm or disarm the timer.
    ///
    /// The timer will be called the next time after the provided `value` duration.
    /// After that, the timer will be repeatedly called again at the the specified `interval`.
    ///
    /// If `interval` is `None` or zero, the timer will only be called once. \
    /// If `value` is `None` or zero, the timer will be disabled.
    ///
    /// # Panics
    /// The provided durations seconds must fit in an i64. Otherwise, this function will panic.
    pub fn update_timer(&self, value: Option<Duration>, interval: Option<Duration>) -> SpaResult {
        fn duration_to_timespec(duration: Duration) -> spa_sys::timespec {
            spa_sys::timespec {
                tv_sec: duration.as_secs().try_into().expect("Duration too long"),
                tv_nsec: duration.subsec_nanos().try_into().unwrap(),
            }
        }

        let value = duration_to_timespec(value.unwrap_or_default());
        let interval = duration_to_timespec(interval.unwrap_or_default());

        let res = unsafe {
            let mut iface = self.loop_.as_raw().utils.as_ref().unwrap().iface;

            spa_interface_call_method!(
                &mut iface as *mut spa_sys::spa_interface,
                spa_sys::spa_loop_utils_methods,
                update_timer,
                self.as_ptr(),
                &value as *const _ as *mut _,
                &interval as *const _ as *mut _,
                false
            )
        };

        SpaResult::from_c(res)
    }
}

impl<'l> IsSource for TimerSource<'l> {
    fn as_ptr(&self) -> *mut spa_sys::spa_source {
        self.ptr.as_ptr()
    }
}

impl<'l> Drop for TimerSource<'l> {
    fn drop(&mut self) {
        unsafe { self.loop_.destroy_source(self) }
    }
}
