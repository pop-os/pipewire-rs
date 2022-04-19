// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

use std::ops::Deref;
use std::ptr;
use std::rc::{Rc, Weak};

use crate::loop_::{self, IsLoop};
use crate::{error::Error, Properties};
use spa::ReadableDict;

#[derive(Debug, Clone)]
pub struct MainLoop {
    inner: Rc<MainLoopInner>,
}

impl MainLoop {
    /// Initialize Pipewire and create a new `MainLoop`
    pub fn new() -> Result<Self, Error> {
        super::init();
        let inner = MainLoopInner::new::<Properties>(None)?;
        Ok(Self {
            inner: Rc::new(inner),
        })
    }

    pub fn with_properties<T: ReadableDict>(properties: &T) -> Result<Self, Error> {
        let inner = MainLoopInner::new(Some(properties))?;
        Ok(Self {
            inner: Rc::new(inner),
        })
    }

    pub fn downgrade(&self) -> WeakMainLoop {
        let weak = Rc::downgrade(&self.inner);
        WeakMainLoop { weak }
    }
}

impl Deref for MainLoop {
    type Target = MainLoopInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl IsLoop for MainLoop {
    fn as_loop(&self) -> &loop_::LoopRef {
        unsafe {
            &*(pw_sys::pw_main_loop_get_loop(self.inner.as_ptr()) as *mut loop_::LoopRef
                as *const _)
        }
    }
}

pub struct WeakMainLoop {
    weak: Weak<MainLoopInner>,
}

impl WeakMainLoop {
    pub fn upgrade(&self) -> Option<MainLoop> {
        self.weak.upgrade().map(|inner| MainLoop { inner })
    }
}

#[derive(Debug)]
pub struct MainLoopInner {
    ptr: ptr::NonNull<pw_sys::pw_main_loop>,
}

impl MainLoopInner {
    fn new<T: ReadableDict>(properties: Option<&T>) -> Result<Self, Error> {
        unsafe {
            let props = properties.map_or(ptr::null(), |props| props.get_dict_ptr()) as *mut _;
            let l = pw_sys::pw_main_loop_new(props);
            let ptr = ptr::NonNull::new(l).ok_or(Error::CreationFailed)?;

            Ok(MainLoopInner { ptr })
        }
    }

    fn as_ptr(&self) -> *mut pw_sys::pw_main_loop {
        self.ptr.as_ptr()
    }

    pub fn run(&self) {
        unsafe {
            pw_sys::pw_main_loop_run(self.as_ptr());
        }
    }

    pub fn quit(&self) {
        unsafe {
            pw_sys::pw_main_loop_quit(self.as_ptr());
        }
    }
}

impl Drop for MainLoopInner {
    fn drop(&mut self) {
        unsafe { pw_sys::pw_main_loop_destroy(self.ptr.as_ptr()) }
    }
}
