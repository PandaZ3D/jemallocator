// Copyright 2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Bindings for jemalloc as an allocator
//!
//! This crate provides bindings to jemalloc as a memory allocator for Rust.
//! This crate mainly exports, one type, `Jemalloc`, which implements the
//! `GlobalAlloc` trait and optionally the `Alloc` trait,
//! and is suitable both as a memory allocator and as a global allocator.

#![cfg_attr(feature = "alloc_trait", feature(allocator_api))]
// TODO: rename the following lint on next minor bump
#![allow(renamed_and_removed_lints)]
#![deny(missing_docs, broken_intra_doc_links)]
#![no_std]

#[cfg(feature = "alloc_trait")]
// use core::alloc::{Alloc, AllocErr, CannotReallocInPlace, Excess};
use core::alloc::{Allocator, AllocError};
use core::alloc::{GlobalAlloc, Layout};
#[cfg(feature = "alloc_trait")]
use core::ptr::NonNull;

use libc::{c_int, c_void};

// This constant equals _Alignof(max_align_t) and is platform-specific. It
// contains the _maximum_ alignment that the memory allocations returned by the
// C standard library memory allocation APIs (e.g. `malloc`) are guaranteed to
// have.
//
// The memory allocation APIs are required to return memory that can fit any
// object whose fundamental aligment is <= _Alignof(max_align_t).
//
// In C, there are no ZSTs, and the size of all types is a multiple of their
// alignment (size >= align). So for allocations with size <=
// _Alignof(max_align_t), the malloc-APIs return memory whose alignment is
// either the requested size if its a power-of-two, or the next smaller
// power-of-two.
#[cfg(any(target_arch = "arm", target_arch = "mips", target_arch = "powerpc"))]
const ALIGNOF_MAX_ALIGN_T: usize = 8;
#[cfg(any(
    target_arch = "x86",
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "powerpc64",
    target_arch = "loongarch64",
    target_arch = "mips64",
    target_arch = "riscv64",
    target_arch = "s390x",
    target_arch = "sparc64"
))]
const ALIGNOF_MAX_ALIGN_T: usize = 16;

/// If `align` is less than `_Alignof(max_align_t)`, and if the requested
/// allocation `size` is larger than the alignment, we are guaranteed to get a
/// suitably aligned allocation by default, without passing extra flags, and
/// this function returns `0`.
///
/// Otherwise, it returns the alignment flag to pass to the jemalloc APIs.
fn layout_to_flags(align: usize, size: usize) -> c_int {
    if align <= ALIGNOF_MAX_ALIGN_T && align <= size {
        0
    } else {
        ffi::MALLOCX_ALIGN(align)
    }
}

// Assumes a condition that always must hold.
macro_rules! assume {
    ($e:expr) => {
        debug_assert!($e);
        if !($e) {
            core::hint::unreachable_unchecked();
        }
    };
}

/// Handle to the jemalloc allocator
///
/// This type implements the `GlobalAllocAlloc` trait, allowing usage a global allocator.
///
/// When the `alloc_trait` feature of this crate is enabled, it also implements the `Alloc` trait,
/// allowing usage in collections.
#[derive(Copy, Clone, Default, Debug)]
pub struct Jemalloc;

unsafe impl GlobalAlloc for Jemalloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        assume!(layout.size() != 0);
        let flags = layout_to_flags(layout.align(), layout.size());
        let ptr = if flags == 0 {
            ffi::malloc(layout.size())
        } else {
            ffi::mallocx(layout.size(), flags)
        };
        ptr as *mut u8
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        assume!(layout.size() != 0);
        let flags = layout_to_flags(layout.align(), layout.size());
        let ptr = if flags == 0 {
            ffi::calloc(1, layout.size())
        } else {
            ffi::mallocx(layout.size(), flags | ffi::MALLOCX_ZERO)
        };
        ptr as *mut u8
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        assume!(!ptr.is_null());
        assume!(layout.size() != 0);
        let flags = layout_to_flags(layout.align(), layout.size());
        ffi::sdallocx(ptr as *mut c_void, layout.size(), flags)
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        assume!(layout.size() != 0);
        assume!(new_size != 0);
        let flags = layout_to_flags(layout.align(), new_size);
        let ptr = if flags == 0 {
            ffi::realloc(ptr as *mut c_void, new_size)
        } else {
            ffi::rallocx(ptr as *mut c_void, new_size, flags)
        };
        ptr as *mut u8
    }
}

#[cfg(feature = "alloc_trait")]
unsafe impl Allocator for Jemalloc {
    // #[inline]
    // unsafe fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, AllocErr> {
    //     NonNull::new(GlobalAlloc::alloc(self, layout)).ok_or(AllocErr)
    // }

    #[inline]
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        unsafe {
            let ptr = GlobalAlloc::alloc(self, layout);
            if ptr.is_null() {
                return Err(AllocError)
            }
            // let slice = core::slice::from_raw_parts(ptr, layout.size());
            // NonNull::new(slice).ok_or(AllocError).map(|ptr| ptr.cast())
            Ok(NonNull::new_unchecked(core::ptr::slice_from_raw_parts_mut(ptr, layout.size())))
        }
    }

    #[inline]
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        GlobalAlloc::dealloc(self, ptr.as_ptr(), layout)
    }
}

/// Return the usable size of the allocation pointed to by ptr.
///
/// The return value may be larger than the size that was requested during allocation.
/// This function is not a mechanism for in-place `realloc()`;
/// rather it is provided solely as a tool for introspection purposes.
/// Any discrepancy between the requested allocation size
/// and the size reported by this function should not be depended on,
/// since such behavior is entirely implementation-dependent.
///
/// # Safety
///
/// `ptr` must have been allocated by `Jemalloc` and must not have been freed yet.
pub unsafe fn usable_size<T>(ptr: *const T) -> usize {
    ffi::malloc_usable_size(ptr as *const c_void)
}

/// Raw bindings to jemalloc
mod ffi {
    pub use tikv_jemalloc_sys::*;
}
