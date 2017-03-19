// Licensed: Apache 2.0

// allow some unused utility functions
#![allow(dead_code)]

use std::ptr;

// Only use 'plain_ptr' for iterating, ideally we could iterate over raw pointers
// and the user could wrap them in 'plain_ptr' if they wanted.
//
// Currently this isn't trivial to do,
// awaiting `impl Trait` to return a mapped iterator directly
// (only in nightly builds as of 1.13).
use plain_ptr::{
    PtrConst,
    PtrMut,
};

pub trait MemPoolElemUtils {
    fn default_chunk_size() -> usize;
    fn free_ptr_get(&self) -> *mut Self;
    fn free_ptr_set(&mut self, ptr: *mut Self);
    // needed for iteration
    fn free_ptr_test(&self) -> bool;
}

pub trait MemElem:
    MemPoolElemUtils +
    {}

impl<TElem> MemElem for TElem where TElem:
    MemPoolElemUtils +
    {}

struct MemChunk<TElem: MemElem> {
    data: Vec<TElem>,
}

pub struct MemPool<TElem: MemElem> {
    chunks: Vec<MemChunk<TElem>>,
    chunk_size: usize,
    // only for book-keeping, not essential
    elem_count: usize,
    // number of elements per chunk
    free: *mut TElem,
}

impl<TElem: MemElem> Default for MemPool<TElem> {
    fn default() -> MemPool<TElem> {
        MemPool::new()
    }
}

impl<TElem: MemElem> MemPool<TElem> {

    // ------------------------------------------------------------------------
    // Internal API

    /// Ensure self.free isn't null
    fn free_elem_ensure(&mut self) {
        if self.free.is_null() {
            let mut chunk: Vec<TElem> = Vec::with_capacity(self.chunk_size);
            unsafe { chunk.set_len(self.chunk_size); }

            // populate free list
            let mut elem_prev: *mut TElem = ptr::null_mut();
            for elem in &mut chunk {
                elem.free_ptr_set(elem_prev);
                elem_prev = elem as *mut TElem;
            }

            self.free = chunk.last_mut().unwrap();

            // avoid running drop, caller needs to manage this!
            unsafe { chunk.set_len(0); }

            self.chunks.push(MemChunk {
                data: chunk,
            });
        }
    }

    pub fn with_chunk_size(chunk_size: usize) -> MemPool<TElem> {
        MemPool {
            chunks: Vec::new(),
            chunk_size: chunk_size,
            elem_count: 0,
            free: ptr::null_mut(),
        }
    }
    pub fn new() -> MemPool<TElem> {
        MemPool::with_chunk_size(TElem::default_chunk_size())
    }

    pub fn len(
        &self,
    ) -> usize {
        return self.elem_count;
    }

    pub fn is_empty(
        &self,
    ) -> bool {
        return self.elem_count == 0;
    }

    pub fn clear(
        &mut self,
    ) {
        // keep a single chunk
        self.chunks.clear();
        self.elem_count = 0;
        self.free = ptr::null_mut();
    }

    pub unsafe fn alloc_elem_uninitialized(
        &mut self,
    ) -> *mut TElem {
        self.elem_count += 1;
        self.free_elem_ensure();
        let elem = self.free;
        self.free = (*elem).free_ptr_get();
        return &mut (*elem);
    }

    pub fn alloc_elem_from(
        &mut self,
        from: TElem,
    ) -> *mut TElem {
        self.elem_count += 1;
        self.free_elem_ensure();
        let elem = self.free;
        self.free = unsafe { (*elem).free_ptr_get() };
        // only difference!
        unsafe {
            ::std::ptr::write(elem, from);
        }
        return unsafe { &mut (*elem) };
    }

    pub fn free_elem(
        &mut self,
        elem: *mut TElem,
    ) {
        self.elem_count -= 1;
        unsafe {
            (*elem).free_ptr_set(self.free);
        }
        self.free = elem;
    }

    // -----------------
    // Utility Functions

    pub fn as_vec_mut(
        &mut self,
    ) -> Vec<*mut TElem> {
        let mut vec = Vec::with_capacity(self.elem_count);
        for c in &mut self.chunks {
            for i in 0..self.chunk_size {
                let elem = unsafe { c.data.get_unchecked_mut(i) };
                if !elem.free_ptr_test() {
                    vec.push(elem as *mut TElem);
                }
            }
        }
        debug_assert!(vec.len() == self.elem_count);
        return vec;
    }

    pub fn as_vec(
        &self,
    ) -> Vec<*const TElem> {
        let mut vec = Vec::with_capacity(self.elem_count);
        for c in &self.chunks {
            for i in 0..self.chunk_size {
                let elem = unsafe { c.data.get_unchecked(i) };
                if !elem.free_ptr_test() {
                    vec.push(elem as *const TElem);
                }
            }
        }
        return vec;
    }

    // ------------------
    // Iterator Functions
    //
    // Helpers for iterator structs,
    // exposed by 'iter' and 'iter_mut' methods.

    fn iter_impl_elem_from_index_ref(&self, pos: &IterPos) -> &TElem {
        debug_assert!(pos.chunk_index < self.chunks.len() && pos.data_index < self.chunk_size);
        return unsafe {
            self.chunks.get_unchecked(
                pos.chunk_index).data.get_unchecked(
                    pos.data_index)
        };
    }

    fn iter_impl_elem_from_index_mut(&mut self, pos: &IterPos) -> *mut TElem {
        debug_assert!(pos.chunk_index < self.chunks.len() && pos.data_index < self.chunk_size);
        return unsafe {
            self.chunks.get_unchecked_mut(
                pos.chunk_index).data.get_unchecked_mut(
                    pos.data_index) as *mut TElem
        };
    }

    fn iter_impl_elem_from_index_const(&self, pos: &IterPos) -> *const TElem {
        debug_assert!(pos.chunk_index < self.chunks.len() && pos.data_index < self.chunk_size);
        return unsafe {
            self.chunks.get_unchecked(
                pos.chunk_index).data.get_unchecked(
                    pos.data_index) as *const TElem
        };
    }

    fn iter_impl_step(&self, pos: &mut IterPos) {
        assert!(pos.chunk_index != ::std::usize::MAX);
        loop {
            pos.data_index = pos.data_index.wrapping_add(1);
            if pos.data_index == self.chunk_size {
                pos.data_index = 0;
                pos.chunk_index = pos.chunk_index.wrapping_add(1);
                if pos.chunk_index == self.chunks.len() {
                    // signal there is no more!
                    pos.chunk_index = ::std::usize::MAX;
                    return;
                }
            }
            if self.iter_impl_elem_from_index_ref(pos).free_ptr_test() == false {
                break;
            }
        }
    }

    fn iter_find_first(&self) -> IterPos {
        if self.elem_count == 0 {
            IterPos {
                chunk_index: ::std::usize::MAX,
                data_index: 0,
            }
        } else {
            // intentionally offset so step wraps back to zero
            let mut pos = IterPos {
                chunk_index: 0,
                data_index: 0_usize.wrapping_sub(1),
            };
            self.iter_impl_step(&mut pos);
            debug_assert!(pos.chunk_index != ::std::usize::MAX);
            pos
        }
    }

    fn iter_to_size_hint(&self, pos: &IterPos) -> (usize, Option<usize>) {
        let count_final = self.elem_count;
        if pos.chunk_index == 0 && pos.data_index == 0 {
            return (count_final, Some(count_final));
        } else {
            use std::cmp::min;
            let count_max = self.chunks.len() * self.chunk_size;
            // Elements covered so far, in the case that none were freed.
            let pos_max = (pos.chunk_index * self.chunk_size) + pos.data_index;
            // Calculate a best guess without keeping exact count while iterating.
            return (
                if pos_max < count_final { count_final.wrapping_sub(pos_max) } else { 0 },
                Some(min(count_max.wrapping_sub(pos_max), count_final)),
            );
        }
    }

    // ------------------
    // Iterators (Public)

    pub fn iter_mut(&mut self) -> MemPoolIterMut<TElem> {
        let pos = self.iter_find_first();
        MemPoolIterMut {
            pool: self,
            pos: pos,
        }
    }

    pub fn iter(&self) -> MemPoolIterConst<TElem> {
        let pos = self.iter_find_first();
        MemPoolIterConst {
            pool: self,
            pos: pos,
        }
    }
}


// ----------------------------------------------------------------------------
// Iterator
//
// Note that `MemPoolIterMut` & `MemPoolIterConst` use exactly the same logic.

/// Current iterator position
struct IterPos {
    chunk_index: usize,
    data_index: usize,
}

pub struct MemPoolIterMut<'a, TElem: MemElem>
    where TElem: 'a
{
    pool: &'a mut MemPool<TElem>,
    /// [chunk_index, data_index]
    pos: IterPos,
}

pub struct MemPoolIterConst<'a, TElem: MemElem>
    where TElem: 'a
{
    pool: &'a MemPool<TElem>,
    /// [chunk_index, data_index]
    pos: IterPos,
}

impl <'a, TElem> Iterator for MemPoolIterConst<'a, TElem>
    where TElem: MemElem,
{
    type Item = PtrConst<TElem>;

    fn next(&mut self) -> Option<PtrConst<TElem>> {
        if self.pos.chunk_index != ::std::usize::MAX {
            let elem = PtrConst(self.pool.iter_impl_elem_from_index_const(&self.pos));
            self.pool.iter_impl_step(&mut self.pos);
            return Some(elem);
        } else {
            return None;
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        return self.pool.iter_to_size_hint(&self.pos);
    }
}

impl <'a, TElem> Iterator for MemPoolIterMut<'a, TElem>
    where TElem: MemElem,
{
    type Item = PtrMut<TElem>;

    #[inline]
    fn next(&mut self) -> Option<PtrMut<TElem>> {
        if self.pos.chunk_index != ::std::usize::MAX {
            let elem = PtrMut(self.pool.iter_impl_elem_from_index_mut(&self.pos));
            self.pool.iter_impl_step(&mut self.pos);
            return Some(elem);
        } else {
            return None;
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        return self.pool.iter_to_size_hint(&self.pos);
    }
}


#[cfg(test)]
#[path="tests_mempool.rs"]
mod test;
