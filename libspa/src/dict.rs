// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! Dictionary types and traits.

use bitflags::bitflags;
// re-exported as used in the static_dict! macro implementation
pub use spa_sys::spa_dict_item;
use std::{ffi::CStr, fmt, marker::PhantomData, ptr};

/// Trait providing API to read dictionaries.
pub trait ReadableDict {
    /// Obtain the pointer to the raw `spa_dict` struct.
    fn get_dict_ptr(&self) -> *const spa_sys::spa_dict;

    /// An iterator over all raw key-value pairs.
    /// The iterator element type is `(&CStr, &CStr)`.
    fn iter_cstr(&self) -> CIter {
        let first_elem_ptr = unsafe { (*self.get_dict_ptr()).items };
        let end = if first_elem_ptr.is_null() {
            ptr::null()
        } else {
            unsafe { first_elem_ptr.offset((*self.get_dict_ptr()).n_items as isize) }
        };

        CIter {
            next: first_elem_ptr,
            end,
            _phantom: PhantomData,
        }
    }

    /// An iterator over all key-value pairs that are valid utf-8.
    /// The iterator element type is `(&str, &str)`.
    fn iter(&self) -> Iter {
        Iter {
            inner: self.iter_cstr(),
        }
    }

    /// An iterator over all keys that are valid utf-8.
    /// The iterator element type is &str.
    fn keys(&self) -> Keys {
        Keys {
            inner: self.iter_cstr(),
        }
    }

    /// An iterator over all values that are valid utf-8.
    /// The iterator element type is &str.
    fn values(&self) -> Values {
        Values {
            inner: self.iter_cstr(),
        }
    }

    /// Returns the number of key-value-pairs in the dict.
    /// This is the number of all pairs, not only pairs that are valid-utf8.
    fn len(&self) -> usize {
        unsafe { (*self.get_dict_ptr()).n_items as usize }
    }

    /// Returns `true` if the dict is empty, `false` if it is not.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the bitflags that are set for the dict.
    fn flags(&self) -> Flags {
        Flags::from_bits_truncate(unsafe { (*self.get_dict_ptr()).flags })
    }

    /// Get the value associated with the provided key.
    ///
    /// If the dict does not contain the key or the value is non-utf8, `None` is returned.
    /// Use [`iter_cstr`] if you need a non-utf8 key or value.
    ///
    /// [`iter_cstr`]: #method.iter_cstr
    fn get(&self, key: &str) -> Option<&str> {
        self.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    /// Get the value associated with the provided key and convert it to a given type.
    ///
    /// If the dict does not contain the key or the value is non-utf8, `None` is returned.
    ///
    /// If the value associated with the key cannot be parsed to the requested type,
    /// `Some(Err(ParseValueError))` is returned.
    ///
    /// See [`ParsableValue#foreign-impls`] for all the types which can be produced by this method.
    ///
    /// # Examples
    /// ```
    /// use libspa::prelude::*;
    /// use libspa::{StaticDict, static_dict};
    ///
    /// static DICT: StaticDict = static_dict! {
    ///     "true" => "true",
    ///     "ten" => "10",
    ///     "pi" => "3.14159265359",
    ///     "pointer" => "pointer:0xdeadbeef"
    /// };
    ///
    /// assert_eq!(DICT.parse("true"), Some(Ok(true)));
    /// assert_eq!(DICT.parse("ten"), Some(Ok(10)));
    /// assert_eq!(DICT.parse("ten"), Some(Ok(10.0)));
    /// assert_eq!(DICT.parse("pi"), Some(Ok(3.14159265359)));
    ///
    /// let ptr = DICT.parse::<*const i32>("pointer").unwrap().unwrap();
    /// assert!(!ptr.is_null());
    /// ```
    fn parse<T: ParsableValue>(&self, key: &str) -> Option<Result<T, ParseValueError>> {
        self.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| match T::parse_value(v) {
                Some(v) => Ok(v),
                None => Err(ParseValueError {
                    value: v.to_string(),
                    type_name: std::any::type_name::<T>(),
                }),
            })
    }

    #[doc(hidden)]
    /// [`Debug`] implementation, should not be used directly by users.
    fn debug(&self, name: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Entries<'a>(CIter<'a>);

        impl<'a> fmt::Debug for Entries<'a> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_map()
                    .entries(self.0.clone().map(|(k, v)| (k, v)))
                    .finish()
            }
        }

        f.debug_struct(name)
            .field("flags", &self.flags())
            .field("entries", &Entries(self.iter_cstr()))
            .finish()
    }
}

/// An error raised by [`ReadableDict::parse`] if the value cannot be converted to the requested type.
#[derive(Debug, PartialEq)]
pub struct ParseValueError {
    value: String,
    type_name: &'static str,
}

impl<'a> std::error::Error for ParseValueError {}

impl<'a> fmt::Display for ParseValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}' cannot be parsed to {}", self.value, self.type_name)
    }
}

/// Trait implemented on types which can be returned by [`ReadableDict::parse`].
pub trait ParsableValue: Copy {
    /// Try parsing `value` to convert it to the requested type.
    fn parse_value(value: &str) -> Option<Self>;
}

impl ParsableValue for bool {
    fn parse_value(value: &str) -> Option<Self> {
        // Same logic as pw_properties_parse_bool()
        if value == "true" {
            Some(true)
        } else {
            match value.parse::<i32>() {
                Ok(1) => Some(true),
                _ => Some(false),
            }
        }
    }
}

macro_rules! impl_parsable_value_numeric {
    ($type_:ty) => {
        impl ParsableValue for $type_ {
            fn parse_value(value: &str) -> Option<Self> {
                value.parse().ok()
            }
        }
    };
}

impl_parsable_value_numeric!(i32);
impl_parsable_value_numeric!(i64);
impl_parsable_value_numeric!(u64);
impl_parsable_value_numeric!(f32);
impl_parsable_value_numeric!(f64);
// not implemented in properties.h but good to have
impl_parsable_value_numeric!(i8);
impl_parsable_value_numeric!(u8);
impl_parsable_value_numeric!(i16);
impl_parsable_value_numeric!(u16);
impl_parsable_value_numeric!(u32);
impl_parsable_value_numeric!(i128);
impl_parsable_value_numeric!(u128);
impl_parsable_value_numeric!(isize);
impl_parsable_value_numeric!(usize);

const POINTER_PREFIX: &str = "pointer:0x";

impl<T> ParsableValue for *const T {
    fn parse_value(value: &str) -> Option<Self> {
        match value
            .strip_prefix(POINTER_PREFIX)
            .map(|addr| usize::from_str_radix(addr, 16))
        {
            Some(Ok(addr)) => Some(addr as *const T),
            _ => None,
        }
    }
}

/// Trait providing API to modify dictionaries.
pub trait WritableDict {
    /// Insert the key-value pair, overwriting any old value.
    fn insert<T: Into<Vec<u8>>>(&mut self, key: T, value: T);

    /// Remove the key-value pair if it exists.
    fn remove<T: Into<Vec<u8>>>(&mut self, key: T);

    /// Clear the object, removing all key-value pairs.
    fn clear(&mut self);
}

/// A wrapper for a `*const spa_dict` struct that does not take ownership of the data,
/// useful for dicts shared to us via FFI.
pub struct ForeignDict {
    ptr: ptr::NonNull<spa_sys::spa_dict>,
}

impl ForeignDict {
    /// Wraps the provided pointer in a read-only `ForeignDict` struct without taking ownership of the struct pointed to.
    ///
    /// # Safety
    ///
    /// - The provided pointer must point to a valid, well-aligned `spa_dict` struct.
    /// - The struct pointed to must be kept valid for the entire lifetime of the created `Dict`.
    ///
    /// Violating any of these rules will result in undefined behaviour.
    pub unsafe fn from_ptr(ptr: ptr::NonNull<spa_sys::spa_dict>) -> Self {
        Self { ptr }
    }
}

impl ReadableDict for ForeignDict {
    fn get_dict_ptr(&self) -> *const spa_sys::spa_dict {
        self.ptr.as_ptr()
    }
}

impl fmt::Debug for ForeignDict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug("ForeignDict", f)
    }
}

bitflags! {
    /// Dictionary flags
    pub struct Flags: u32 {
        // These flags are redefinitions from
        // https://gitlab.freedesktop.org/pipewire/pipewire/-/blob/master/spa/include/spa/utils/dict.h
        /// Dictionary has been sorted.
        const SORTED = spa_sys::SPA_DICT_FLAG_SORTED;
    }
}

/// Iterator on a dictionary's keys and values exposed as [`CStr`].
#[derive(Clone)]
pub struct CIter<'a> {
    next: *const spa_sys::spa_dict_item,
    /// Points to the first element outside of the allocated area, or null for empty dicts
    end: *const spa_sys::spa_dict_item,
    _phantom: PhantomData<&'a str>,
}

impl<'a> Iterator for CIter<'a> {
    type Item = (&'a CStr, &'a CStr);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.next.is_null() && self.next < self.end {
            let k = unsafe { CStr::from_ptr((*self.next).key) };
            let v = unsafe { CStr::from_ptr((*self.next).value) };
            self.next = unsafe { self.next.add(1) };
            Some((k, v))
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let bound: usize = unsafe { self.next.offset_from(self.end) as usize };

        // We know the exact value, so lower bound and upper bound are the same.
        (bound, Some(bound))
    }
}

/// Iterator on a dictionary's keys and values exposed as [`str`].
pub struct Iter<'a> {
    inner: CIter<'a>,
}

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .find_map(|(k, v)| k.to_str().ok().zip(v.to_str().ok()))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // Lower bound is 0, as all keys left might not be valid UTF-8.
        (0, self.inner.size_hint().1)
    }
}

/// Iterator on a dictionary's values.
pub struct Keys<'a> {
    inner: CIter<'a>,
}

impl<'a> Iterator for Keys<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.find_map(|(k, _)| k.to_str().ok())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

/// Iterator on a dictionary's values.
pub struct Values<'a> {
    inner: CIter<'a>,
}

impl<'a> Iterator for Values<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.find_map(|(_, v)| v.to_str().ok())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

/// A collection of static key/value pairs.
///
/// # Examples
/// Create a `StaticDict` and access the stored values by key:
/// ```rust
/// use libspa::prelude::*;
/// use libspa::{StaticDict, static_dict};
///
/// static DICT: StaticDict = static_dict!{
///     "Key" => "Value",
///     "OtherKey" => "OtherValue"
/// };
///
/// assert_eq!(Some("Value"), DICT.get("Key"));
/// assert_eq!(Some("OtherValue"), DICT.get("OtherKey"));
/// ```
pub struct StaticDict {
    ptr: ptr::NonNull<spa_sys::spa_dict>,
}

impl StaticDict {
    /// Create a [`StaticDict`] from an existing raw `spa_dict` pointer.
    ///
    /// # Safety
    /// - The provided pointer must point to a valid, well-aligned `spa_dict` struct.
    /// - The struct and its content need to stay alive during the whole lifetime of the `StaticDict`.
    /// - The keys and values stored in this dict has to be static strings.
    pub const unsafe fn from_ptr(ptr: ptr::NonNull<spa_sys::spa_dict>) -> Self {
        Self { ptr }
    }
}

/// A macro for creating a new [`StaticDict`] with predefined key-value pairs.
///
/// The macro accepts a list of static `Key => Value` pairs, seperated by commas.
///
/// # Examples:
/// Create a `StaticDict`.
/// ```rust
/// use libspa::{StaticDict, static_dict};
///
/// static PROPS: StaticDict = static_dict!{
///    "Key1" => "Value1",
///    "Key2" => "Value2",
/// };
/// ```
#[macro_export]
macro_rules! static_dict {
    {$($k:expr => $v:expr),+ $(,)?} => {{
        use $crate::dict::{spa_dict_item, StaticDict, Flags};
        use std::ptr;

        const ITEMS: &[spa_dict_item] = &[
            $(
                spa_dict_item {
                    key: concat!($k, "\0").as_ptr() as *const std::os::raw::c_char,
                    value: concat!($v, "\0").as_ptr() as *const std::os::raw::c_char
                },
            )+
        ];

        const RAW: spa_sys::spa_dict = spa_sys::spa_dict {
            flags: Flags::empty().bits(),
            n_items: ITEMS.len() as u32,
            items: ITEMS.as_ptr(),
        };

        unsafe {
            let ptr = &RAW as *const _ as *mut _;
            StaticDict::from_ptr(ptr::NonNull::new_unchecked(ptr))
        }
    }};
}

impl ReadableDict for StaticDict {
    fn get_dict_ptr(&self) -> *const spa_sys::spa_dict {
        self.ptr.as_ptr()
    }
}

impl fmt::Debug for StaticDict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug("StaticDict", f)
    }
}

unsafe impl Send for StaticDict {}
unsafe impl Sync for StaticDict {}

#[cfg(test)]
mod tests {
    use super::{Flags, ForeignDict, ReadableDict, StaticDict};
    use spa_sys::spa_dict;
    use std::{ffi::CString, ptr};

    #[test]
    fn test_empty_dict() {
        let raw = spa_dict {
            flags: Flags::empty().bits,
            n_items: 0,
            items: ptr::null(),
        };

        let dict = unsafe { ForeignDict::from_ptr(ptr::NonNull::from(&raw)) };
        let iter = dict.iter_cstr();

        assert_eq!(0, dict.len());

        iter.for_each(|_| panic!("Iterated over non-existing item"));
    }

    #[test]
    fn test_iter_cstr() {
        let dict = static_dict! {
            "K0" => "V0",
            "K1" => "V1"
        };

        let mut iter = dict.iter_cstr();
        assert_eq!(
            (
                CString::new("K0").unwrap().as_c_str(),
                CString::new("V0").unwrap().as_c_str()
            ),
            iter.next().unwrap()
        );
        assert_eq!(
            (
                CString::new("K1").unwrap().as_c_str(),
                CString::new("V1").unwrap().as_c_str()
            ),
            iter.next().unwrap()
        );
        assert_eq!(None, iter.next());
    }

    #[test]
    fn test_iterators() {
        let dict = static_dict! {
            "K0" => "V0",
            "K1" => "V1"
        };

        let mut iter = dict.iter();
        assert_eq!(("K0", "V0"), iter.next().unwrap());
        assert_eq!(("K1", "V1"), iter.next().unwrap());
        assert_eq!(None, iter.next());

        let mut key_iter = dict.keys();
        assert_eq!("K0", key_iter.next().unwrap());
        assert_eq!("K1", key_iter.next().unwrap());
        assert_eq!(None, key_iter.next());

        let mut val_iter = dict.values();
        assert_eq!("V0", val_iter.next().unwrap());
        assert_eq!("V1", val_iter.next().unwrap());
        assert_eq!(None, val_iter.next());
    }

    #[test]
    fn test_get() {
        let dict = static_dict! {
            "K0" => "V0"
        };

        assert_eq!(Some("V0"), dict.get("K0"));
    }

    #[test]
    fn test_debug() {
        let dict = static_dict! {
            "K0" => "V0"
        };

        assert_eq!(
            r#"StaticDict { flags: (empty), entries: {"K0": "V0"} }"#,
            &format!("{:?}", dict)
        );

        let raw = spa_dict {
            flags: Flags::SORTED.bits,
            n_items: 0,
            items: ptr::null(),
        };

        let dict = unsafe { ForeignDict::from_ptr(ptr::NonNull::from(&raw)) };

        assert_eq!(
            r#"ForeignDict { flags: SORTED, entries: {} }"#,
            &format!("{:?}", dict)
        );
    }

    #[test]
    fn static_dict() {
        static DICT: StaticDict = static_dict! {
            "K0" => "V0",
            "K1" => "V1"
        };

        assert_eq!(DICT.len(), 2);
        assert_eq!(DICT.get("K0"), Some("V0"));
        assert_eq!(DICT.get("K1"), Some("V1"));
    }

    #[test]
    fn parse() {
        use super::ParseValueError;

        static DICT: StaticDict = static_dict! {
            "true" => "true",
            "false" => "false",
            "1" => "1",
            "10" => "10",
            "-10" => "-10",
            "i64-max" => "9223372036854775807",
            "1.5" => "1.5",
            "-1.5" => "-1.5",
            "pointer" => "pointer:0xdeadbeef",
            "badger" => "badger"
        };

        macro_rules! parse_error {
            ($key:literal, $type_:ty) => {
                assert!(matches!(
                    DICT.parse::<$type_>($key),
                    Some(Err(ParseValueError { .. }))
                ));
            };
        }

        assert_eq!(DICT.parse::<bool>("missing"), None);

        assert_eq!(DICT.parse("true"), Some(Ok(true)));
        assert_eq!(DICT.parse("1"), Some(Ok(true)));
        assert_eq!(DICT.parse("false"), Some(Ok(false)));
        assert_eq!(DICT.parse("10"), Some(Ok(false)));
        assert_eq!(DICT.parse("badger"), Some(Ok(false)));

        /* integer types */
        assert_eq!(DICT.parse::<i32>("1"), Some(Ok(1)));
        assert_eq!(DICT.parse::<i32>("-10"), Some(Ok(-10)));
        parse_error!("badger", i32);
        parse_error!("i64-max", i32);

        assert_eq!(DICT.parse::<i64>("1"), Some(Ok(1)));
        assert_eq!(DICT.parse::<i64>("-10"), Some(Ok(-10)));
        assert_eq!(DICT.parse::<i64>("i64-max"), Some(Ok(i64::MAX)));
        parse_error!("badger", i64);

        assert_eq!(DICT.parse::<u64>("1"), Some(Ok(1)));
        assert_eq!(DICT.parse::<u64>("i64-max"), Some(Ok(i64::MAX as u64)));
        parse_error!("-10", u64);
        parse_error!("badger", u64);

        assert_eq!(DICT.parse::<i8>("1"), Some(Ok(1)));
        assert_eq!(DICT.parse::<i8>("-10"), Some(Ok(-10)));

        assert_eq!(DICT.parse::<u8>("1"), Some(Ok(1)));
        parse_error!("-10", u8);

        assert_eq!(DICT.parse::<i16>("1"), Some(Ok(1)));
        assert_eq!(DICT.parse::<i16>("-10"), Some(Ok(-10)));

        assert_eq!(DICT.parse::<u16>("1"), Some(Ok(1)));
        parse_error!("-10", u16);

        assert_eq!(DICT.parse::<u32>("1"), Some(Ok(1)));
        parse_error!("-10", u32);

        assert_eq!(DICT.parse::<i128>("1"), Some(Ok(1)));
        assert_eq!(DICT.parse::<i128>("-10"), Some(Ok(-10)));

        assert_eq!(DICT.parse::<u128>("1"), Some(Ok(1)));
        parse_error!("-10", u128);

        assert_eq!(DICT.parse::<isize>("1"), Some(Ok(1)));
        assert_eq!(DICT.parse::<isize>("-10"), Some(Ok(-10)));

        assert_eq!(DICT.parse::<usize>("1"), Some(Ok(1)));
        parse_error!("-10", usize);

        /* floating-point types */
        assert_eq!(DICT.parse::<f32>("1"), Some(Ok(1.0)));
        assert_eq!(DICT.parse::<f32>("-10"), Some(Ok(-10.0)));
        assert_eq!(DICT.parse::<f32>("1.5"), Some(Ok(1.5)));
        assert_eq!(DICT.parse::<f32>("-1.5"), Some(Ok(-1.5)));
        parse_error!("badger", f32);

        assert_eq!(DICT.parse::<f64>("1"), Some(Ok(1.0)));
        assert_eq!(DICT.parse::<f64>("-10"), Some(Ok(-10.0)));
        assert_eq!(DICT.parse::<f64>("1.5"), Some(Ok(1.5)));
        assert_eq!(DICT.parse::<f64>("-1.5"), Some(Ok(-1.5)));
        parse_error!("badger", f64);

        /* pointer */
        let ptr = DICT.parse::<*const i32>("pointer").unwrap().unwrap();
        assert!(!ptr.is_null());
        parse_error!("badger", *const i32);
    }
}
