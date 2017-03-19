// Licensed: Apache 2.0

// Memory for these data types is owned externally

// mod plain_ptr;

// allow some unused utility functions
#![allow(dead_code)]

use ::plain_ptr::{
    PtrMut,
    PtrConst,
    null_mut,
    // null_const,
};

macro_rules! into_expand {
    ($($names:ident),*) => {
        $(let $names = ::std::convert::Into::into($names);)*
    };
}

macro_rules! swap_links {
    ($a:expr, $b:expr) => {
        {
            let mut a = $a;
            let mut b = $b;
            {
                let t = a.prev_get();
                a.prev_set(b.prev_get());
                b.prev_set(t);
            }
            {
                let t = a.next_get();
                a.next_set(b.next_get());
                b.next_set(t);
            }
        }
    }
}


pub trait ListBaseElemUtils
    where
    Self: Sized,
{
    fn next_get(&self) -> PtrMut<Self>;
    fn prev_get(&self) -> PtrMut<Self>;

    fn next_set(&mut self, ptr: PtrMut<Self>);
    fn prev_set(&mut self, ptr: PtrMut<Self>);
}

pub trait LinkElem:
    ListBaseElemUtils +
    {}

impl<LElem> LinkElem for LElem where LElem:
    ListBaseElemUtils +
    {}

#[repr(C)]
pub struct ListBase<LElem: LinkElem> {
    pub head: PtrMut<LElem>,
    pub tail: PtrMut<LElem>,
}

impl<LElem: LinkElem> Default for ListBase<LElem> {
    fn default() -> ListBase<LElem> {
        ListBase {
            head: null_mut(),
            tail: null_mut(),
        }
    }
}

impl <LElem: LinkElem> ListBase<LElem> {

    pub fn new() -> ListBase<LElem> {
        ListBase {
            head: null_mut(),
            tail: null_mut(),
        }
    }

    pub fn push_front(&mut self, mut link: PtrMut<LElem>) {
        link.next_set(self.head);
        link.prev_set(null_mut());
        if !self.head.is_null() {
            self.head.prev_set(link);
        }
        if self.tail.is_null() {
            self.tail = link;
        }
        self.head = link;
    }

    pub fn push_back(&mut self, mut link: PtrMut<LElem>) {
        link.next_set(null_mut());
        link.prev_set(self.tail);
        if !self.tail.is_null() {
            self.tail.next_set(link);
        }
        if self.head.is_null() {
            self.head = link;
        }
        self.tail = link;
    }

    pub fn push_after(&mut self, _prev_link: PtrMut<LElem>, _link: PtrMut<LElem>) {
        unimplemented!();
    }

    pub fn push_before(&mut self, _prev_next: PtrMut<LElem>, _link: PtrMut<LElem>) {
        unimplemented!();
    }

    /// Move all elements from `other` into `self`, leaving `other` empty.
    pub fn append(&mut self, other: &mut ListBase<LElem>) {
        if other.head.is_null() {
            return;
        }

        if self.head.is_null() {
            self.head = other.head;
            self.tail = other.tail;
        } else {
            self.tail.next_set(other.head);
            other.head.prev_set(self.tail);
            self.tail = other.tail;
	    }
	    other.head = null_mut();
        other.tail = null_mut();
    }

    pub fn remove(&mut self, link: PtrMut<LElem>) {
        if !link.next_get().is_null() {
            link.next_get().prev_set(link.prev_get());
        }
        if !link.prev_get().is_null() {
            link.prev_get().next_set(link.next_get());
        }
        if self.tail == link {
            self.tail = link.prev_get();
        }
        if self.head == link {
            self.head = link.next_get();
        }
    }

    pub fn remove_checked(&mut self, link: PtrMut<LElem>) -> bool {
        if self.contains(link) {
            self.remove(link);
            return true;
        } else {
            return false;
        }
    }

    pub fn pop_front(&mut self) -> Option<PtrMut<LElem>> {
        let link = self.head;
        if !link.is_null() {
            self.remove(link);
            return Some(link);
        } else {
            return None;
        }
    }

    pub fn pop_back(&mut self) -> Option<PtrMut<LElem>> {
        let link = self.tail;
        if !link.is_null() {
            self.remove(link);
            return Some(link);
        } else {
            return None;
        }
    }

    pub fn clear(&mut self) {
        self.head = null_mut();
        self.tail = null_mut();
    }

    pub fn len_calc(&self) -> usize {
        let mut len = 0;
        let mut link = self.head;
        while !link.is_null() {
            link = link.next_get();
            len += 1;
        }
        return len;
    }

    pub fn len_calc_ex(&self, len_max: usize) -> usize {
        let mut len = 0;
        let mut link = self.head;
        while !link.is_null() && len != len_max {
            link = link.next_get();
            len += 1;
        }
        return len;
    }

    pub fn is_empty(&self) -> bool {
        return self.head.is_null();
    }

    pub fn is_single(&self) -> bool {
        return !self.head.is_null() && (self.head == self.tail)
    }

    pub fn at_index(&self, index: usize) -> Option<PtrConst<LElem>> {
        let mut index_step = 0;
        let mut link = self.head;
        while !link.is_null() {
            if index_step == index {
                return Some(link.as_const());
            }
            link = link.next_get();
            index_step += 1;
        }
        return None;
    }

    pub fn at_index_mut(&mut self, index: usize) -> Option<PtrMut<LElem>> {
        let mut index_step = 0;
        let mut link = self.head;
        while !link.is_null() {
            if index_step == index {
                return Some(link);
            }
            link = link.next_get();
            index_step += 1;
        }
        return None;
    }

    pub fn index_at<L>(&self, link_find: L) -> Option<usize>
        where L: Into<PtrConst<LElem>>,
    {
        into_expand!(link_find);
        let mut index_step = 0;
        let mut link = self.head;
        while !link.is_null() {
            if link == link_find {
                return Some(index_step);
            }
            link = link.next_get();
            index_step += 1;
        }
        return None;
    }

    pub fn contains<L>(&self, link_find: L) -> bool
        where L: Into<PtrConst<LElem>>,
    {
        into_expand!(link_find);
        let mut link = self.head;
        while !link.is_null() {
            if link == link_find {
                return true;
            }
            link = link.next_get();
        }
        return false;
    }

    /// Put `l_src` in the position of `l_dst`, then remove `l_dst`.
    ///
    /// * `l_dst` *must* be in the list.
    /// * `l_src` *must not* be in the list.
    pub fn replace(&mut self, mut l_src: PtrMut<LElem>, l_dst: PtrMut<LElem>) {
        // close l_src's links
        if !l_src.next_get().is_null() {
            l_src.next_get().prev_set(l_src.prev_get());
        }
        if !l_src.prev_get().is_null() {
            l_src.prev_get().next_set(l_src.next_get());
        }

        // update adjacent links
        if !l_dst.next_get().is_null() {
            l_dst.next_get().prev_set(l_src);
        }
        if !l_dst.prev_get().is_null() {
            l_dst.prev_get().next_set(l_src);
        }

        // set direct links
        l_src.next_set(l_dst.next_get());
        l_src.prev_set(l_dst.prev_get());

        // update list
        if self.head == l_dst {
            self.head = l_src;
        }
        if self.tail == l_dst {
            self.tail = l_src;
        }
    }

    /// Swap a pair of elements, both must be in the list.
    pub fn swap_links(&mut self, a: PtrMut<LElem>, b: PtrMut<LElem>) {
        let (mut a, mut b) = {
            if b.next_get() == a {
                (b, a)
            } else {
                (a, b)
            }
        };

        if a.next_get() == b {
            a.next_set(b.next_get());
            b.prev_set(a.prev_get());
            a.prev_set(b);
            b.next_set(a);
        } else {
            // Non-contiguous items, we can safely swap.
            swap_links!(a, b);
        }

        // Update neighbors of a and b.
        if a.prev_get() != null_mut() {
            a.prev_get().next_set(a);
        }
        if a.next_get() != null_mut() {
            a.next_get().prev_set(a);
        }
        if b.prev_get() != null_mut() {
            b.prev_get().next_set(b);
        }
        if b.next_get() != null_mut() {
            b.next_get().prev_set(b);
        }

        if self.tail == a {
            self.tail = b;
        } else if self.tail == b {
            self.tail = a;
        }

        if self.head == a {
            self.head = b;
        } else if self.head == b {
            self.head = a;
        }
    }

    pub fn reverse(&mut self) {
        let mut curr: PtrMut<LElem> = self.head;
        let mut prev: PtrMut<LElem> = null_mut();
        while !curr.is_null() {
            let next = curr.next_get();
            curr.next_set(prev);
            curr.prev_set(next);
            prev = curr;
            curr = next;
        }

        // swap head/tail
        curr = self.head;
        self.head = self.tail;
        self.tail = curr;
    }

    // ------------------
    // Iterators (Public)

    pub fn iter_mut(&mut self) -> ListBaseIterMut<LElem> {
        let link_iter = self.head;
        ListBaseIterMut {
            _list: self,
            link_iter: link_iter,
        }
    }
    pub fn iter(&self) -> ListBaseIterConst<LElem> {
        let link_iter = self.head;
        ListBaseIterConst {
            _list: self,
            link_iter: link_iter,
        }
    }
}


// ----------------------------------------------------------------------------
// Iterator
//
// Nope, many more functions could be implemented
// (double-ended for reverse, peekable... etc)

pub struct ListBaseIterMut<'a, LElem: LinkElem>
    where LElem: 'a
{
    _list: &'a mut ListBase<LElem>,
    link_iter: PtrMut<LElem>,
}

pub struct ListBaseIterConst<'a, LElem: LinkElem>
    where LElem: 'a
{
    _list: &'a ListBase<LElem>,
    link_iter: PtrMut<LElem>,
}

impl <'a, LElem> Iterator for ListBaseIterMut<'a, LElem>
    where LElem: LinkElem,
{
    type Item = PtrMut<LElem>;

    #[inline]
    fn next(&mut self) -> Option<PtrMut<LElem>> {
        if !self.link_iter.is_null() {
            let elem = self.link_iter;
            self.link_iter = self.link_iter.next_get();
            return Some(elem);
        } else {
            return None;
        }
    }
}

impl <'a, LElem> Iterator for ListBaseIterConst<'a, LElem>
    where LElem: LinkElem,
{
    type Item = PtrConst<LElem>;

    #[inline]
    fn next(&mut self) -> Option<PtrConst<LElem>> {
        if !self.link_iter.is_null() {
            let elem = self.link_iter;
            self.link_iter = self.link_iter.next_get();
            return Some(elem.as_const());
        } else {
            return None;
        }
    }
}
