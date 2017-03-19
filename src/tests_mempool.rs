// Apache License, Version 2.0
// (c) Campbell Barton, 2016

use std::ptr;
use mempool_elem::{
    MemPool,
    MemPoolElemUtils,
};

struct TestElem {
    value: usize,
    link: *mut TestElem,
    is_free: bool,
}

impl MemPoolElemUtils for TestElem {
    #[inline] fn default_chunk_size() -> usize {
        return 0; // don't run!
    }
    #[inline] fn free_ptr_get(&self) -> *mut TestElem {
        return self.link;
    }
    #[inline] fn free_ptr_set(&mut self, ptr: *mut TestElem) {
        self.link = ptr;
    }
    #[inline] fn free_ptr_test(&self) -> bool {
        self.is_free
    }
}

impl Default for TestElem {
    fn default() -> TestElem {
        TestElem {
            value: 0,
            link: ptr::null_mut(),
            is_free: false,
        }
    }
}

#[test]
fn test_mempool() {
    let total = 128;
    let chunk_size = 2;
    let mut p: MemPool<TestElem> = MemPool::with_chunk_size(chunk_size);

    for _ in 0..2 {
        let mut a = unsafe { &mut *p.alloc_elem_from(Default::default()) };
        a.value = 0;
        for i in 1..total {
            let a_next = p.alloc_elem_from(Default::default());
            let a_prev = a;
            a = unsafe { &mut *a_next };
            a.value = i;
            a.link = a_prev;
        }

        for i in (0..total).rev() {
            assert!(a.value == i);
            let a_next = unsafe { &mut *a.link };
            p.free_elem(a);
            a = a_next;
        }
    }
}
