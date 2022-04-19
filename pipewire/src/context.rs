// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

use std::{os::unix::prelude::RawFd, ptr};

use crate::core_::Core;
use crate::error::Error;
use crate::loop_::IsLoop;
use crate::properties::Properties;

#[derive(Debug)]
pub struct Context<T: IsLoop + Clone> {
    ptr: ptr::NonNull<pw_sys::pw_context>,
    /// Store the loop here, so that the loop is not dropped before the context, which may lead to
    /// undefined behaviour.
    _loop: T,
}

impl<T: IsLoop + Clone> Context<T> {
    fn new_internal(loop_: &T, properties: Option<Properties>) -> Result<Self, Error> {
        let props = properties.map_or(ptr::null(), |props| props.into_raw()) as *mut _;
        let context = unsafe {
            pw_sys::pw_context_new(loop_.as_loop().as_raw() as *const _ as *mut _, props, 0)
        };
        let context = ptr::NonNull::new(context).ok_or(Error::CreationFailed)?;

        Ok(Context {
            ptr: context,
            _loop: loop_.clone(),
        })
    }

    pub fn new(loop_: &T) -> Result<Self, Error> {
        Self::new_internal(loop_, None)
    }

    pub fn with_properties(loop_: &T, properties: Properties) -> Result<Self, Error> {
        Self::new_internal(loop_, Some(properties))
    }

    fn as_ptr(&self) -> *mut pw_sys::pw_context {
        self.ptr.as_ptr()
    }

    pub fn connect(&self, properties: Option<Properties>) -> Result<Core, Error> {
        let properties = properties.map_or(ptr::null_mut(), |p| p.into_raw());

        unsafe {
            let core = pw_sys::pw_context_connect(self.as_ptr(), properties, 0);
            let ptr = ptr::NonNull::new(core).ok_or(Error::CreationFailed)?;

            Ok(Core::from_ptr(ptr))
        }
    }

    pub fn connect_fd(&self, fd: RawFd, properties: Option<Properties>) -> Result<Core, Error> {
        let properties = properties.map_or(ptr::null_mut(), |p| p.into_raw());

        unsafe {
            let core = pw_sys::pw_context_connect_fd(self.as_ptr(), fd, properties, 0);
            let ptr = ptr::NonNull::new(core).ok_or(Error::CreationFailed)?;

            Ok(Core::from_ptr(ptr))
        }
    }
}

impl<T: IsLoop + Clone> Drop for Context<T> {
    fn drop(&mut self) {
        unsafe { pw_sys::pw_context_destroy(self.as_ptr()) }
    }
}
