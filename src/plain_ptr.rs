// Licensed: Apache 2.0

///
/// Wrap raw pointers
/// This is effectively syntax sugar to avoid;
/// `(*var).attr`, allowing `var.attr` on raw pointers.
///
/// Other utility functions may be exposed,
/// however the intent is to keep this a thin wrapper on raw pointers,
/// not to invent a new way of managing pointers.
///
/// Notes:
///
/// * Guaranteed to have *zero* memory and run-time overhead for release builds.
/// * Supports comparison with regular pointers,
///   convenient for comparing with `std::ptr::null()`.
/// * Supports converting `PtrMut` 'into' `PtrConst`,
///   so functions can be declared which take either.
/// * Can be used as much or as little as your like,
///   To get the value from a pointer: `plain_ptr = PtrMut(ptr)`
///   To get the original pointer: `plain_ptr.as_ptr()`
///

use std::ops::{
    Deref,
    DerefMut,
};


// ---------------------------------------------------------------------------
// Generics (PtrAny)

pub trait PtrAnyImpl<T> {
    /// Either `*mut T` or `*const T`
    type BasePtr;

    fn new(ptr: Self::BasePtr) -> Self;
    /// Gives a native type, from a `mut`.
    /// Beware: this is a workaround for not being able to easily coerce types
    /// when using `PtrAny`.
    fn new_from_mut(ptr: PtrMut<T>) -> Self;
    fn null() -> Self;
    fn is_null(&self) -> bool;
    fn as_ptr(&self) -> Self::BasePtr;
    fn as_const(&self) -> PtrConst<T>;
    /// Utility function to support easy null pointer assignments:
    /// `if let Some(var) = func_returns_pointer() { ... }`
    fn as_option(&self) -> Option<Self> where Self: Sized;
}

pub trait PtrAny<T>:
    Deref<Target=T> +
    Copy +
    Clone +
    PartialEq +
    PtrAnyImpl<T> +
    {}

impl<TPtr, T> PtrAny<T> for TPtr where TPtr:
    Deref<Target=T> +
    Copy +
    Clone +
    PartialEq +
    PtrAnyImpl<T> +
    {}


// ---------------------------------------------------------------------------
// PtrMut

#[repr(C)]
#[derive(Debug, Hash)]
pub struct PtrMut<T> {
    ptr: *mut T,
}

// only for easy access
#[allow(non_snake_case)]
pub fn PtrMut<T>(ptr: *mut T) -> PtrMut<T> {
    PtrMut::new(ptr)
}
pub fn null_mut<T>() -> PtrMut<T> {
    PtrMut::null()
}

impl<T> PtrAnyImpl<T> for PtrMut<T> {
    type BasePtr = *mut T;

    // classmethods
    #[inline(always)]
    fn new(ptr: Self::BasePtr) -> PtrMut<T> {
        PtrMut::new(ptr)
    }
    #[inline(always)]
    fn new_from_mut(ptr: PtrMut<T>) -> PtrMut<T> {
        PtrMut::new_from_mut(ptr)
    }
    #[inline(always)]
    fn null() -> PtrMut<T> {
        PtrMut { ptr: ::std::ptr::null_mut() }
    }

    // methods
    #[inline(always)]
    fn is_null(&self) -> bool {
        self.is_null()
    }
    #[inline(always)]
    fn as_ptr(&self) -> Self::BasePtr {
        self.as_ptr()
    }
    #[inline(always)]
    fn as_option(&self) -> Option<PtrMut<T>> {
        self.as_option()
    }
    #[inline(always)]
    fn as_const(&self) -> PtrConst<T> {
        self.as_const()
    }
}

// PtrAnyImpl
impl<T> PtrMut<T> {
    // classmethods
    #[inline(always)]
    pub fn new(ptr: *mut T) -> PtrMut<T> {
        PtrMut { ptr: ptr as *mut T }
    }
    #[inline(always)]
    fn new_from_mut(ptr: PtrMut<T>) -> PtrMut<T> {
        ptr
    }
    #[inline(always)]
    fn null() -> PtrMut<T> {
        PtrMut { ptr: ::std::ptr::null_mut() }
    }

    // methods
    #[inline(always)]
    pub fn is_null(&self) -> bool {
        self.ptr == ::std::ptr::null_mut()
    }
    #[inline(always)]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr
    }
    #[inline(always)]
    pub fn as_option(&self) -> Option<PtrMut<T>> {
        return if self.ptr.is_null() == false { Some(*self) } else { None };
    }
}

impl<T> PtrMut<T> {

    /// Utility function to support easy null pointer assignments:
    /// `if let Some(var) = func_returns_pointer() { ... }`

    // only for 'PtrMut'
    #[inline(always)]
    pub fn as_const(&self) -> PtrConst<T> {
        PtrConst::new(self.ptr as *const T)
    }
}

impl<T> Copy for PtrMut<T> { }
impl<T> Clone for PtrMut<T> {
    #[inline(always)]
    fn clone(&self) -> PtrMut<T> { *self }
}

impl<T> Deref for PtrMut<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        unsafe { &*self.ptr }
    }
}

impl<T> DerefMut for PtrMut<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.ptr }
    }
}

// Expose other helpers

impl<T> PartialEq for PtrMut<T> {
    fn eq(&self, other: &PtrMut<T>) -> bool {
        self.ptr == other.ptr
    }
}

impl<T> PartialEq<PtrConst<T>> for PtrMut<T> {
    fn eq(&self, other: &PtrConst<T>) -> bool {
        self.ptr as *const T == other.ptr
    }
}

// PtrMut == *mut
impl<T> PartialEq<*mut T> for PtrMut<T> {
    fn eq(&self, other: &*mut T) -> bool {
        self.ptr == *other
    }
}
// PtrMut == *const
impl<T> PartialEq<*const T> for PtrMut<T> {
    fn eq(&self, other: &*const T) -> bool {
        self.ptr as *const T == *other
    }
}

// PtrMut order
impl<T> PartialOrd<PtrMut<T>> for PtrMut<T> {
    fn partial_cmp(&self, other: &PtrMut<T>) -> Option<::std::cmp::Ordering> {
        (self.ptr as usize).partial_cmp(&((other.ptr) as usize))
    }
}
impl<T> Ord for PtrMut<T> {
    fn cmp(&self, other: &Self) -> ::std::cmp::Ordering {
        (self.ptr as usize).cmp(&((other.ptr) as usize))
    }
}
impl<T> Eq for PtrMut<T> {}


// ---------------------------------------------------------------------------
// PtrConst

#[repr(C)]
#[derive(Debug, Hash)]
pub struct PtrConst<T> {
    ptr: *const T,
}

// only for easy access
#[allow(non_snake_case)]
pub fn PtrConst<T>(ptr: *const T) -> PtrConst<T> {
    PtrConst::new(ptr)
}
pub fn null_const<T>() -> PtrConst<T> {
    PtrConst::null()
}

impl<T> PtrAnyImpl<T> for PtrConst<T> {
    type BasePtr = *const T;

    #[inline(always)]
    fn new(ptr: Self::BasePtr) -> PtrConst<T> {
        PtrConst::new(ptr)
    }
    #[inline(always)]
    fn new_from_mut(ptr: PtrMut<T>) -> PtrConst<T> {
        PtrConst::new_from_mut(ptr)
    }
    #[inline(always)]
    fn null() -> PtrConst<T> {
        PtrConst::null()
    }

    #[inline(always)]
    fn is_null(&self) -> bool {
        self.is_null()
    }
    #[inline(always)]
    fn as_ptr(&self) -> Self::BasePtr {
        self.as_ptr()
    }
    #[inline(always)]
    fn as_option(&self) -> Option<PtrConst<T>> {
        self.as_option()
    }
    #[inline(always)]
    fn as_const(&self) -> PtrConst<T> {
        self.as_const()
    }

}

// PtrAnyImpl
impl<T> PtrConst<T> {
    // classmethods
    #[inline(always)]
    fn new(ptr: *const T) -> PtrConst<T> {
        PtrConst { ptr: ptr as *const T }
    }
    #[inline(always)]
    fn new_from_mut(ptr: PtrMut<T>) -> PtrConst<T> {
        ptr.as_const()
    }
    #[inline(always)]
    fn null() -> PtrConst<T> {
        PtrConst { ptr: ::std::ptr::null() }
    }

    // methods
    #[inline(always)]
    fn is_null(&self) -> bool {
        self.ptr == ::std::ptr::null()
    }
    #[inline(always)]
    pub fn as_ptr(&self) -> *const T {
        self.ptr
    }
    #[inline(always)]
    pub fn as_option(&self) -> Option<PtrConst<T>> {
        return if self.ptr.is_null() == false { Some(*self) } else { None };
    }

    #[inline(always)]
    fn as_const(&self) -> PtrConst<T> { PtrConst::new(self.ptr) }
}

impl<T> PtrConst<T> {

    /// Only for 'PtrConst'
    ///
    /// Unlike other functions in this module that are _not_ marked unsafe,
    /// this is something that should really be avoided,
    /// since const-correctness should be maintained strictly
    /// (that's why we have `PtrConst` and `PtrMut`).
    ///
    /// This is needed the case when we have a function which
    /// returns a values who's mutable state is based on the input.
    ///
    /// This way we can avoid writing it twice,
    /// by writing the immutable version once (which will be assured not to modify the input)
    /// then write a mutable wrapper that gets the output
    /// and performs the unsafe case on the output.
    ///
    /// Later it may be worth trying to use generic functions here,
    /// but for now allow unsafe casting.
    ///
    #[inline(always)]
    #[allow(dead_code)]
    pub unsafe fn as_mut(&self) -> PtrMut<T> {
        PtrMut::new(self.ptr as *mut T)
    }
}

impl<T> Copy for PtrConst<T> { }
impl<T> Clone for PtrConst<T> {
    #[inline(always)]
    fn clone(&self) -> PtrConst<T> { *self }
}

impl<T> Deref for PtrConst<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        unsafe { &*self.ptr }
    }
}

// no DerefMut for PtrConst, only PtrMut
/*
impl<T> DerefMut for PtrConst<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.ptr }
    }
}
*/

// Expose other helpers

impl<T> PartialEq for PtrConst<T> {
    fn eq(&self, other: &PtrConst<T>) -> bool {
        self.ptr == other.ptr
    }
}

impl<T> PartialEq<PtrMut<T>> for PtrConst<T> {
    fn eq(&self, other: &PtrMut<T>) -> bool {
        self.ptr == other.ptr as *const T
    }
}

// PtrConst == *mut
impl<T> PartialEq<*mut T> for PtrConst<T> {
    fn eq(&self, other: &*mut T) -> bool {
        self.ptr == *other
    }
}

// PtrConst == *const
impl<T> PartialEq<*const T> for PtrConst<T> {
    fn eq(&self, other: &*const T) -> bool {
        self.ptr == *other
    }
}

// PtrConst order
impl<T> PartialOrd<PtrConst<T>> for PtrConst<T> {
    fn partial_cmp(&self, other: &PtrConst<T>) -> Option<::std::cmp::Ordering> {
        (self.ptr as usize).partial_cmp(&((other.ptr) as usize))
    }
}
impl<T> Ord for PtrConst<T> {
    fn cmp(&self, other: &Self) -> ::std::cmp::Ordering {
        (self.ptr as usize).cmp(&((other.ptr) as usize))
    }
}
impl<T> Eq for PtrConst<T> {}

impl<T> From<PtrMut<T>> for PtrConst<T> {
    fn from(value: PtrMut<T>) -> PtrConst<T> {
        PtrConst::new(value.ptr)
    }
}

