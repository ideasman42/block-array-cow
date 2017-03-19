// Licensed: Apache 2.0

// allow some unused utility functions
#![allow(dead_code)]

const MULTIPLIER: u64 = 0x5DEECE66D_u64;
const MASK: u64 = 0x0000FFFFFFFFFFFF_u64;
// #define MASK_BYTES   2

const ADDEND: u64 = 0xB;
const LOWSEED: u64 = 0x330E;

pub struct Rng {
    pub x: u64,
}

impl Rng {

    #[inline]
    fn seed_value(seed: u32) -> u64 {
        // ((seed as u64) << 16) | LOWSEED
        (seed as u64).wrapping_shl(16) | LOWSEED
    }
    #[inline]
    fn step_value(x: u64) -> u64 {
        // (MULTIPLIER * x + ADDEND) & MASK
        MULTIPLIER.wrapping_mul(x).wrapping_add(ADDEND) & MASK
    }

    pub fn new(seed: u32) -> Self {
        Rng {
            x: Rng::seed_value(seed),
        }
    }

    pub fn seed(&mut self, seed: u32) {
        self.x = Rng::seed_value(seed);
    }

    pub fn step(&mut self) {
        self.x = Rng::step_value(self.x);
    }

    pub fn skip(&mut self, n: usize) {
        for _ in 0..n {
            self.step();
        }
    }

    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        let len = slice.len();
        for i in 0..len {
            let j = (self.x as usize) % len;
            if i != j {
                slice.swap(i, j);
            }
        }
    }

    pub fn fill<T: RandGen>(&mut self, slice: &mut [T]) {
        for v in slice {
            *v = T::rand_value(self);
        }
    }

    pub fn get<T: RandGen>(&mut self) -> T {
        T::rand_value(self)
    }

    pub fn get_vec<T: RandGen>(&mut self, len: usize) -> Vec<T> {
        let mut v: Vec<T> = Vec::with_capacity(len);
        unsafe { v.set_len(len) };
        self.fill(&mut v[..]);
        return v;
    }
}

#[inline]
pub fn slice_u8_from_any_mut<T: Sized>(p: &mut T) -> &mut [u8] {
    unsafe {
        ::std::slice::from_raw_parts_mut((p as *mut T) as *mut u8, ::std::mem::size_of::<T>())
    }
}

pub trait RandGen {
    fn rand_value(r: &mut Rng) -> Self;
}

macro_rules! rand_gen_byte_impl {
    ($($t:ty)*) => ($(
        impl RandGen for $t {
            #[inline]
            fn rand_value(r: &mut Rng) -> Self {
                r.step();
                (r.x % 256_u64) as $t
            }
        }
    )*)
}

macro_rules! rand_gen_any_impl {
    ($($t:ty)*) => ($(
        impl RandGen for $t {
            #[inline]
            fn rand_value(r: &mut Rng) -> Self {
                let mut v: $t = unsafe { ::std::mem::uninitialized() };
                r.fill(slice_u8_from_any_mut(&mut v));
                v
            }
        }
    )*)
}

macro_rules! rand_gen_float_impl {
    ($($t:ty)*) => ($(
        impl RandGen for $t {
            #[inline]
            fn rand_value(r: &mut Rng) -> Self {
                (u32::rand_value(r) as $t) / 0x80000000_u64 as $t
            }
        }
    )*)
}

macro_rules! rand_gen_int32_impl {
    ($($t:ty)*) => ($(
        impl RandGen for $t {
            #[inline]
            fn rand_value(r: &mut Rng) -> Self {
                r.step();
                // (r.x >> 17) as i32
                r.x.wrapping_shr(17) as $t
            }
        }
    )*)
}

rand_gen_byte_impl! {
    i8 u8
}

rand_gen_any_impl! {
    i16 i64 isize
    u16 u64 usize
}

rand_gen_float_impl! {
    f32 f64
}

rand_gen_int32_impl! {
    i32 u32
}
