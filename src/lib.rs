// Apache License, Version 2.0
// (c) Blender Foundation, 2016
//     Campbell Barton, 2017

//! Array storage to minimize duplication.
//!
//! This is done by splitting arrays into chunks and using copy-on-write (COW),
//! to de-duplicate chunks,
//! from the users perspective this is an implementation detail.
//!
//! # Overview
//!
//! ## Data Structure
//!
//! This diagram is an overview of the structure of a single array-store.
//!
//! note: The only 2 structures here which are referenced externally are the.
//!
//! * `BArrayStore`: The whole array store.
//! * `BArrayState`: Represents a single state (array) of data.
//!   These can be add using a reference state,
//!   while this could be considered the previous or parent state.
//!   no relationship is kept,
//!   so the caller is free to add any state from the same `BArrayStore` as a reference.
//!
//! ```.text
//! <+> BArrayStore: root data-structure,
//!  |  can store many 'states', which share memory.
//!  |
//!  |  This can store many arrays, however they must share the same 'stride'.
//!  |  Arrays of different types will need to use a new BArrayStore.
//!  |
//!  +- <+> states (Collection of BArrayState's):
//!  |   |  Each represents an array added by the user of this API.
//!  |   |  and references a chunk_list (each state is a chunk_list user).
//!  |   |  Note that the list order has no significance.
//!  |   |
//!  |   +- <+> chunk_list (BChunkList):
//!  |       |  The chunks that make up this state.
//!  |       |  Each state is a chunk_list user,
//!  |       |  avoids duplicating lists when there is no change between states.
//!  |       |
//!  |       +- chunk_refs (List of BChunkRef): Each chunk_ref links to a a BChunk.
//!  |          Each reference is a chunk user,
//!  |          avoids duplicating smaller chunks of memory found in multiple states.
//!  |
//!  +- info (BArrayInfo):
//!  |  Sizes and offsets for this array-store.
//!  |  Also caches some variables for reuse.
//!  |
//!  +- <+> memory (BArrayMemory):
//!      |  Memory pools for storing BArrayStore data.
//!      |
//!      +- chunk_list (Pool of BChunkList):
//!      |  All chunk_lists, (reference counted, used by BArrayState).
//!      |
//!      +- chunk_ref (Pool of BChunkRef):
//!      |  All chunk_refs (link between BChunkList & BChunk).
//!      |
//!      +- chunks (Pool of BChunk):
//!         All chunks, (reference counted, used by BChunkList).
//!         These have their headers hashed for reuse so we can quickly check for duplicates.
//! ```
//!
//!
//! ### De-Duplication
//!
//! When creating a new state, a previous state can be given as a reference,
//! matching chunks from this state are re-used in the new state.
//!
//! First matches at either end of the array are detected.
//! For identical arrays this is all thats needed.
//!
//! De-duplication is performed on any remaining chunks,
//! by hashing the first few bytes of the chunk
//! (see: `BCHUNK_HASH_TABLE_ACCUMULATE_STEPS`).
//!
//! \note This is cached for reuse since the referenced data never changes.
//!
//! An array is created to store hash values at every 'stride',
//! then stepped over to search for matching chunks.
//!
//! Once a match is found, there is a high chance next chunks match too,
//! so this is checked to avoid performing so many hash-lookups.
//! Otherwise new chunks are created.
//!
//! # Example
//!
//! ```
//! let mut bs = block_array_cow::BArrayStore::new(1, 8);
//! let data_src_a = b"The quick brown fox jumps over the lazy dog";
//! let data_src_b = b"The quick brown fox almost jumps over the lazy dog";
//! let data_src_c = b"The little quick brown fox jumps over the lazy dog!";
//!
//! let state_a = bs.state_add(data_src_a, None);
//! let state_b = bs.state_add(data_src_b, Some(state_a));
//! let state_c = bs.state_add(data_src_c, Some(state_b));
//!
//! // Check the data is stored correctly
//! let data_dst = block_array_cow::BArrayStore::state_data_get_alloc(state_a);
//! assert_eq!(&data_src_a[..], &data_dst[..]);
//!
//! let data_dst = block_array_cow::BArrayStore::state_data_get_alloc(state_b);
//! assert_eq!(&data_src_b[..], &data_dst[..]);
//!
//! let data_dst = block_array_cow::BArrayStore::state_data_get_alloc(state_c);
//! assert_eq!(&data_src_c[..], &data_dst[..]);
//! ```


// -----------------------------------------------------------------------------
// Constants

/// # Defines
///
/// Some of the logic for merging is quite involved,
/// support disabling some parts of this.

/// Scan first chunks (happy path when beginning of the array matches).
/// When the array is a perfect match, we can re-use the entire list.
///
/// Note that disabling makes some tests fail that check for output-size.
const USE_FASTPATH_CHUNKS_FIRST: bool = true;

/// Scan last chunks (happy path when end of the array matches).
/// When the end of the array matches, we can quickly add these chunks.
///
/// Note that we will add contiguous matching chunks
/// so this isn't as useful as `USE_FASTPATH_CHUNKS_FIRST`,
/// however it avoids adding matching chunks into the lookup table,
/// so creating the lookup table won't be as expensive.
const USE_FASTPATH_CHUNKS_LAST: bool = USE_FASTPATH_CHUNKS_FIRST;

/// For arrays of matching length, test that *enough* of the chunks are aligned,
/// and simply step over both arrays, using matching chunks.
/// This avoids overhead of using a lookup table for cases
/// when we can assume they're mostly aligned.
const USE_ALIGN_CHUNKS_TEST: bool = true;

/// Number of times to propagate hashes back.
/// Effectively a 'triangle-number'.
/// so 4 -> 7, 5 -> 10, 6 -> 15... etc.
const BCHUNK_HASH_TABLE_ACCUMULATE_STEPS: usize = 4;

/// Calculate the key once and reuse it
const HASH_TABLE_KEY_UNSET: u64 = ::std::u64::MAX;
const HASH_TABLE_KEY_FALLBACK: u64 = ::std::u64::MAX - 1;

/// How much larger the table is then the total number of chunks.
const BCHUNK_HASH_TABLE_MUL: usize = 3;

/// Merge too small/large chunks:
///
/// Using this means chunks below a threshold will be merged together.
/// Even though short term this uses more memory,
/// long term the overhead of maintaining many small chunks is reduced.
/// This is defined by setting the minimum chunk size
/// (as a fraction of the regular chunk size).
///
/// Chunks may also become too large (when incrementally growing an array),
/// this also enables chunk splitting.
const USE_MERGE_CHUNKS: bool = true;

/// `ifdef USE_MERGE_CHUNKS`
/// Merge chunks smaller then: `(chunk_size / BCHUNK_MIN_SIZE_DIV)`
///
const BCHUNK_SIZE_MIN_DIV: usize = 8;

/// Disallow chunks bigger then the regular chunk size scaled by this value
///
/// note: must be at least 2!
/// however, this code runs wont run in tests unless its ~1.1 ugh.
/// so lower only to check splitting works.
const BCHUNK_SIZE_MAX_MUL: usize = 2;
/// USE_MERGE_CHUNKS

/// slow (keep disabled), but handy for debugging
const USE_VALIDATE_LIST_SIZE: bool = false;

const USE_VALIDATE_LIST_DATA_PARTIAL: bool = false;

const USE_PARANOID_CHECKS: bool = false;

const MEMPOOL_CHUNK_SIZE: usize = 512;

// -----------------------------------------------------------------------------
// Modules

mod plain_ptr;
use plain_ptr::{
    PtrMut,
    PtrConst,

    null_mut,
    null_const,
};

mod mempool_elem;
use mempool_elem::{
    MemPool,
    MemPoolElemUtils,
};

mod list_base;
use list_base::{
    ListBase,
    ListBaseElemUtils,
};

use ::std::cmp::{
    min,
    max,
};

/// NOP for now, keep since this may be supported later.
macro_rules! unlikely {
    ($body:expr) => {
        $body
    }
}

// -----------------------------------------------------------------------------
// Internal Structs

type HashKey = u64;

struct BArrayInfo {
    chunk_stride: usize,
    // chunk_count: usize, // UNUSED

    // pre-calculated
    chunk_byte_size: usize,
    // min/max limits (inclusive)
    chunk_byte_size_min: usize,
    chunk_byte_size_max: usize,

    accum_read_ahead_bytes: usize,
    accum_steps: usize,
    accum_read_ahead_len: usize,
}

struct BArrayMemory {
    state: MemPool<BArrayState>,
    chunk_list: MemPool<BChunkList>,
    chunk_ref: MemPool<BChunkRef>,
    // this needs explicit drop on it's 'data'
    chunk: MemPool<BChunk>,
}

///
/// Main storage for all states
///
pub struct BArrayStore {
    // static data
    info: BArrayInfo,

    // memory storage
    memory: BArrayMemory,

    // `BArrayState` may be in any order
    // (logic should never depend on state order).
    states: ListBase<BArrayState>,
}


///
/// A single instance of an array.
///
/// This is how external API's hold a reference to an in-memory state,
/// although the struct is private.
///
pub struct BArrayState {
    // linked list in `BArrayStore.states`
    next: PtrMut<BArrayState>,
    prev: PtrMut<BArrayState>,

    // BChunkList's
    chunk_list: PtrMut<BChunkList>,
}

struct BChunkList {
    // BChunkRef's
    chunk_refs: ListBase<BChunkRef>,
    // ListBase.count(chunks), store for reuse.
    chunk_refs_len: usize,
    // size of all chunks
    total_size: usize,

    // number of `BArrayState` using this.
    users: isize,
}

/// A chunk of an array.
struct BChunk {
    data: Vec<u8>,

    // number of `BChunkList` using this.
    users: isize,

    key: HashKey,
}

/// Links to store `BChunk` data in `BChunkList.chunks`.
struct BChunkRef {
    next: PtrMut<BChunkRef>,
    prev: PtrMut<BChunkRef>,
    link: PtrMut<BChunk>,
}

///
/// Single linked list used when putting chunks into a temporary table,
/// used for lookups.
///
/// Point to the `BChunkRef`, not the `BChunk`,
/// to allow talking down the chunks in-order until a mis-match is found,
/// this avoids having to do so many table lookups.
///
struct BTableRef {
    next: PtrMut<BTableRef>,
    cref: PtrMut<BChunkRef>,
}

/// internal structs


// -----------------------------------------------------------------------------
// MemPoolElemUtils impl

macro_rules! mempool_list_elem_impl {
    ($t:ty) => {
        impl MemPoolElemUtils for $t {
            #[inline] fn default_chunk_size() -> usize {
                MEMPOOL_CHUNK_SIZE
            }
            #[inline] fn free_ptr_get(&self) -> *mut Self {
                return self.next.as_ptr() as usize as *mut Self;
            }
            #[inline] fn free_ptr_set(&mut self, ptr: *mut Self) {
                self.next = ::plain_ptr::PtrMut(ptr as usize as *mut _);
                self.prev = PtrMut(self);
            }
            #[inline] fn free_ptr_test(&self) -> bool {
                self.prev == PtrConst(self)
            }
        }
    }
}

mempool_list_elem_impl!(BArrayState);
mempool_list_elem_impl!(BChunkRef);

impl MemPoolElemUtils for BChunkList {
    #[inline] fn default_chunk_size() -> usize {
        MEMPOOL_CHUNK_SIZE
    }
    #[inline] fn free_ptr_get(&self) -> *mut Self {
        return self.chunk_refs.head.as_ptr() as usize as *mut Self;
    }
    #[inline] fn free_ptr_set(&mut self, ptr: *mut Self) {
        self.chunk_refs.head = PtrMut(ptr as usize as *mut _);
        self.chunk_refs.tail = PtrMut((self as *const _) as usize as *mut _);
    }
    #[inline] fn free_ptr_test(&self) -> bool {
        self.chunk_refs.tail.as_ptr() as usize == (self as *const _ as usize)
    }
}

impl MemPoolElemUtils for BChunk {
    #[inline] fn default_chunk_size() -> usize {
        MEMPOOL_CHUNK_SIZE
    }
    #[inline] fn free_ptr_get(&self) -> *mut Self {
        return self.users as *mut Self;
    }
    #[inline] fn free_ptr_set(&mut self, ptr: *mut Self) {
        self.users = ptr as isize;
        self.key = self as *const _ as HashKey;
    }
    #[inline] fn free_ptr_test(&self) -> bool {
        self.key == self as *const _ as HashKey
    }
}


// -----------------------------------------------------------------------------
// ListBaseElemUtils impl

macro_rules! list_base_elem_impl {
    ($t:ty) => {
        impl ListBaseElemUtils for $t {
            #[inline] fn next_get(&self) -> PtrMut<Self> { self.next }
            #[inline] fn prev_get(&self) -> PtrMut<Self> { self.prev }
            #[inline] fn next_set(&mut self, ptr: PtrMut<Self>) { self.next = ptr; }
            #[inline] fn prev_set(&mut self, ptr: PtrMut<Self>) { self.prev = ptr; }
        }
    }
}

list_base_elem_impl!(BArrayState);
list_base_elem_impl!(BChunkRef);


// -----------------------------------------------------------------------------
// Internal API

// put internal API in its own module

/// # Internal BChunk API
/// []( { )

fn bchunk_new(
    bs_mem: &mut BArrayMemory, data: Vec<u8>,
) -> PtrMut<BChunk> {
    PtrMut(bs_mem.chunk.alloc_elem_from(
        BChunk {
            data: data,
            users: 0,
            key: HASH_TABLE_KEY_UNSET,
        }
    ))
}

fn bchunk_new_copydata(
    bs_mem: &mut BArrayMemory, data: &[u8],
) -> PtrMut<BChunk> {
    let mut data_copy = Vec::with_capacity(data.len());
    data_copy.extend_from_slice(data);
    return bchunk_new(bs_mem, data_copy);
}

fn bchunk_decref(
    bs_mem: &mut BArrayMemory, mut chunk: PtrMut<BChunk>,
) {
    debug_assert!(chunk.users > 0);
    if chunk.users == 1 {
        unsafe { ::std::ptr::drop_in_place(&mut chunk.data) };
        bs_mem.chunk.free_elem(chunk.as_ptr());
    } else {
        chunk.users -= 1;
    }
}

fn bchunk_data_compare(
    chunk: PtrMut<BChunk>,
    data_base: &[u8],
    data_base_len: usize,
    offset: usize,
) -> bool {
    if offset + chunk.data.len() <= data_base_len {
        return &data_base[offset..(offset + chunk.data.len())] == &chunk.data[..];
    } else {
        return false;
    }
}

/// []( } )

/// # Internal BChunkList API
/// []( { )

fn bchunk_list_new(
    bs_mem: &mut BArrayMemory,
    total_size: usize,
) -> PtrMut<BChunkList> {
    PtrMut(bs_mem.chunk_list.alloc_elem_from(
        BChunkList {
            chunk_refs: ListBase::new(),
            chunk_refs_len: 0,
            total_size: total_size,
            users: 0,
        }
    ))
}

fn bchunk_list_decref(
    bs_mem: &mut BArrayMemory, mut chunk_list: PtrMut<BChunkList>,
) {
    debug_assert!(chunk_list.users > 0);
    if chunk_list.users == 1 {
        let mut cref = chunk_list.chunk_refs.head;
        while cref != null_mut() {
            let cref_next = cref.next;
            bchunk_decref(bs_mem, cref.link);
            bs_mem.chunk_ref.free_elem(cref.as_ptr());
            cref = cref_next;
        }

        bs_mem.chunk_list.free_elem(chunk_list.as_ptr());
    } else {
        chunk_list.users -= 1;
    }
}

macro_rules! debug_assert_chunklist_size {
    ($chunk_list:expr, $n:expr) => {
        {
            if USE_VALIDATE_LIST_SIZE {
                debug_assert_eq!(bchunk_list_size($chunk_list), $n)
            }
        }
    }
}

// USE_VALIDATE_LIST_DATA_PARTIAL
fn bchunk_list_data_check(
    chunk_list: PtrMut<BChunkList>, data: &[u8],
) -> bool {
    let mut offset = 0;
    for cref in chunk_list.chunk_refs.iter() {
        if &data[offset..(offset + cref.link.data.len())] != &cref.link.data[..] {
            return false;
        }
        offset += cref.link.data.len();
    }
    return true;
}

macro_rules! debug_assert_chunklist_data {
    ($chunk_list:expr, $data:expr) => {
        {
            if USE_VALIDATE_LIST_DATA_PARTIAL {
                debug_assert!(bchunk_list_data_check($chunk_list, $data));
            }
        }
    }
}

// USE_MERGE_CHUNKS
fn bchunk_list_ensure_min_size_last(
    info: &BArrayInfo, bs_mem: &mut BArrayMemory,
    mut chunk_list: PtrMut<BChunkList>,
) {
    let mut cref = chunk_list.chunk_refs.tail;
    if cref != null_mut() && cref.prev != null_mut() {
        // both are decref'd after use (end of this block)
        let chunk_curr: PtrMut<BChunk> = cref.link;
        let chunk_prev: PtrMut<BChunk> = cref.prev.link;

        if min(chunk_prev.data.len(), chunk_curr.data.len()) < info.chunk_byte_size_min {
            let data_merge_len = chunk_prev.data.len() + chunk_curr.data.len();
            // we could pass, but no need
            if data_merge_len <= info.chunk_byte_size_max {
                // we have enough space to merge

                // remove last from linklist
                debug_assert!(chunk_list.chunk_refs.tail != chunk_list.chunk_refs.head);
                cref.prev.next = null_mut();
                chunk_list.chunk_refs.tail = cref.prev;
                chunk_list.chunk_refs_len -= 1;

                let mut data_merge: Vec<u8> = Vec::with_capacity(data_merge_len);
                data_merge.extend_from_slice(&chunk_prev.data[..]);
                data_merge.extend_from_slice(&chunk_curr.data[..]);

                cref.prev.link = bchunk_new(bs_mem, data_merge);
                cref.prev.link.users += 1;
                bs_mem.chunk_ref.free_elem(cref.as_ptr());
            } else {
                // If we always merge small slices,
                // we should _almost_ never end up having very large chunks.
                // Gradual expanding on contracting will cause this.
                //
                // if we do, the code below works (test by setting 'BCHUNK_SIZE_MAX_MUL = 1.2')

                // keep chunk on the left hand side a regular size
                let split = info.chunk_byte_size;

                // merge and split
                let data_prev_len = split;
                let data_curr_len = data_merge_len - split;
                let mut data_prev: Vec<u8> = Vec::with_capacity(data_prev_len);
                let mut data_curr: Vec<u8> = Vec::with_capacity(data_curr_len);

                if data_prev_len <= chunk_prev.data.len() {
                    // setup 'data_prev'
                    data_prev.extend_from_slice(&chunk_prev.data[..]);

                    // setup 'data_curr'
                    data_curr.extend_from_slice(
                        &chunk_prev.data[data_prev_len..chunk_prev.data.len()]);
                    data_curr.extend_from_slice(
                        &chunk_curr.data[..]);
                } else {
                    debug_assert!(data_curr_len <= chunk_curr.data.len());
                    debug_assert!(data_prev_len >= chunk_prev.data.len());

                    let data_prev_grow_len = data_prev_len - chunk_prev.data.len();

                    // setup 'data_prev'
                    data_prev.extend_from_slice(&chunk_prev.data[..]);
                    data_prev.extend_from_slice(&chunk_curr.data[0..data_prev_grow_len]);

                    // setup 'data_curr'
                    data_curr.extend_from_slice(
                        &chunk_curr.data[data_prev_grow_len..(data_prev_grow_len + data_curr_len)]);
                }

                debug_assert_eq!(data_prev_len, data_prev.len());
                debug_assert_eq!(data_curr_len, data_curr.len());

                cref.prev.link = bchunk_new(bs_mem, data_prev);
                cref.prev.link.users += 1;

                cref.link = bchunk_new(bs_mem, data_curr);
                cref.link.users += 1;
            }

            // free zero users
            bchunk_decref(bs_mem, chunk_curr);
            bchunk_decref(bs_mem, chunk_prev);
        }
    }
}

/// Return length split into 2 values: (usize, usize)
///
/// * `data_trim_len` Length which is aligned to the `BArrayInfo.chunk_byte_size`.
/// * `data_last_chunk_len` The remaining bytes.
///
/// Note: This function ensures the size of `data_last_chunk_len`
/// is larger than `BArrayInfo.chunk_byte_size_min`.
fn bchunk_list_calc_trim_len(
    info: &BArrayInfo, data_len: usize,
) -> (usize, usize) {
    let mut data_last_chunk_len: usize;
    let mut data_trim_len: usize = data_len;

    if USE_MERGE_CHUNKS {
        // avoid creating too-small chunks
        // more efficient then merging after
        if data_len > info.chunk_byte_size {
            data_last_chunk_len = data_trim_len % info.chunk_byte_size;
            data_trim_len = data_trim_len - data_last_chunk_len;
            if data_last_chunk_len != 0 {
                if data_last_chunk_len < info.chunk_byte_size_min {
                    // may be zero and thats OK
                    data_trim_len -= info.chunk_byte_size;
                    data_last_chunk_len += info.chunk_byte_size;
                }
            }
        } else {
            data_trim_len = 0;
            data_last_chunk_len = data_len;
        }

        debug_assert!((data_trim_len == 0) || (data_trim_len >= info.chunk_byte_size));
    } else {
        data_last_chunk_len = data_trim_len % info.chunk_byte_size;
        data_trim_len = data_trim_len - data_last_chunk_len;
    }

    debug_assert_eq!(data_trim_len + data_last_chunk_len, data_len);

    (data_trim_len, data_last_chunk_len)
}

/// Append and don't manage merging small chunks.
fn bchunk_list_append_only(
    bs_mem: &mut BArrayMemory,
    mut chunk_list: PtrMut<BChunkList>, mut chunk: PtrMut<BChunk>,
) {
    let cref = PtrMut(bs_mem.chunk_ref.alloc_elem_from(
        BChunkRef {
            next: null_mut(),
            prev: null_mut(),
            link: chunk,
        })
    );
    chunk_list.chunk_refs.push_back(cref);
    chunk_list.chunk_refs_len += 1;
    chunk.users += 1
}

/// note: This is for writing single chunks,
/// use `bchunk_list_append_data_n` when writing large blocks of memory into many chunks.
fn bchunk_list_append_data(
    info: &BArrayInfo, bs_mem: &mut BArrayMemory,
    chunk_list: PtrMut<BChunkList>,
    data: &[u8],
) {
    debug_assert!(data.len() != 0);

    if USE_MERGE_CHUNKS {
        debug_assert!(data.len() <= info.chunk_byte_size_max);

        if !chunk_list.chunk_refs.is_empty() {
            let mut cref: PtrMut<BChunkRef> = chunk_list.chunk_refs.tail;
            let chunk_prev: PtrMut<BChunk> = cref.link;
            if min(chunk_prev.data.len(), data.len()) < info.chunk_byte_size_min {
                let data_merge_len = chunk_prev.data.len() + data.len();
                // realloc for single user
                if cref.link.users == 1 {
                    cref.link.data.extend_from_slice(data);
                } else {
                    let mut data_merge: Vec<u8> = Vec::with_capacity(data_merge_len);
                    data_merge.extend_from_slice(&chunk_prev.data[..]);
                    data_merge.extend_from_slice(data);
                    cref.link = bchunk_new(bs_mem, data_merge);
                    cref.link.users += 1;
                    bchunk_decref(bs_mem, chunk_prev);
                }
                debug_assert_eq!(data_merge_len, cref.link.data.len());
                return;
            }
        }
    }

    let chunk: PtrMut<BChunk> = bchunk_new_copydata(bs_mem, data);
    bchunk_list_append_only(bs_mem, chunk_list, chunk);

    // don't run this, instead preemptively avoid creating a chunk only to merge it (above).
    if false && USE_MERGE_CHUNKS {
        bchunk_list_ensure_min_size_last(info, bs_mem, chunk_list);
    }
}

/// Similar to `bchunk_list_append_data`, but handle multiple chunks.
/// Use for adding arrays of arbitrary sized memory at once.
///
/// Note: this function takes care not to perform redundant chunk-merging checks,
/// so we can write successive fixed size chunks quickly.
fn bchunk_list_append_data_n(
    info: &BArrayInfo, bs_mem: &mut BArrayMemory,
    chunk_list: PtrMut<BChunkList>,
    data: &[u8],
) {
    let (data_trim_len, data_last_chunk_len) = bchunk_list_calc_trim_len(info, data.len());

    if data_trim_len != 0 {
        let mut i_prev;

        {
            let i = info.chunk_byte_size;
            bchunk_list_append_data(info, bs_mem, chunk_list, &data[0..i]);
            i_prev = i;
        }

        while i_prev != data_trim_len {
            let i = i_prev + info.chunk_byte_size;
            let chunk = bchunk_new_copydata(bs_mem, &data[i_prev..i]);
            bchunk_list_append_only(bs_mem, chunk_list, chunk);
            i_prev = i;
        }

        if data_last_chunk_len != 0 {
            let chunk = bchunk_new_copydata(
                bs_mem, &data[i_prev..(i_prev + data_last_chunk_len)]);
            bchunk_list_append_only(bs_mem, chunk_list, chunk);
            // i_prev = data.len();  // UNUSED
        }
    } else {
        // if we didn't write any chunks previously,
        // we may need to merge with the last.
        if data_last_chunk_len != 0 {
            debug_assert_eq!(data.len(), data_last_chunk_len);
            bchunk_list_append_data(info, bs_mem, chunk_list, data);
            // i_prev = data.len();  // UNUSED
        }
    }

    if USE_MERGE_CHUNKS {
        if data.len() > info.chunk_byte_size {
            debug_assert!(chunk_list.chunk_refs.tail.link.data.len() >= info.chunk_byte_size_min);
        }
    }
}

fn bchunk_list_append(
    info: &BArrayInfo, bs_mem: &mut BArrayMemory,
    chunk_list: PtrMut<BChunkList>,
    chunk: PtrMut<BChunk>,
) {
    bchunk_list_append_only(bs_mem, chunk_list, chunk);

    if USE_MERGE_CHUNKS {
        bchunk_list_ensure_min_size_last(info, bs_mem, chunk_list);
    }
}

fn bchunk_list_fill_from_array(
    info: &BArrayInfo, bs_mem: &mut BArrayMemory,
    chunk_list: PtrMut<BChunkList>,
    data: &[u8],
) {
    debug_assert!(chunk_list.chunk_refs.is_empty());
    let (data_trim_len, data_last_chunk_len) = bchunk_list_calc_trim_len(info, data.len());

    let mut i_prev = 0;
    while i_prev != data_trim_len {
        let i = i_prev + info.chunk_byte_size;
        let chunk = bchunk_new_copydata(bs_mem, &data[i_prev..i]);
        bchunk_list_append_only(bs_mem, chunk_list, chunk);
        i_prev = i;
    }

    if data_last_chunk_len != 0 {
        let chunk = bchunk_new_copydata(bs_mem, &data[i_prev..(i_prev + data_last_chunk_len)]);
        bchunk_list_append_only(bs_mem, chunk_list, chunk);
        // i_prev = data.len();
    }

    if USE_MERGE_CHUNKS {
        if data.len() > info.chunk_byte_size {
            debug_assert!(chunk_list.chunk_refs.tail.link.data.len() >= info.chunk_byte_size_min);
        }
    }

    // works but better avoid redundant re-alloc
    if false && USE_MERGE_CHUNKS {
        bchunk_list_ensure_min_size_last(info, bs_mem, chunk_list);
    }

    debug_assert_chunklist_size!(chunk_list, data.len());
    debug_assert_chunklist_data!(chunk_list, data);
}


// ---------------------------------------------------------------------------
// Internal Table Lookup Functions

/// # Internal Hashing/De-Duplication API
///
/// Only used by `bchunk_list_from_data_merge`.

const HASH_INIT: u32 = 5381;

#[inline]
fn hash_data_single(p: u8) -> u32 {
    return ((HASH_INIT << 5) + HASH_INIT).wrapping_add((p as i8) as u32);
}

// hash bytes
fn hash_data(key: &[u8]) -> u32 {
    let mut h: u32 = HASH_INIT;

    for p in key {
        // h = (h << 5) + h + ((*p as i8) as u32);
        h = h.wrapping_shl(5).wrapping_add(h).wrapping_add((*p as i8) as u32);
    }

    return h;
}

fn hash_array_from_data(
    info: &BArrayInfo, data_slice: &[u8],
    hash_array: &mut [HashKey],
) {
    if info.chunk_stride != 1 {
        let mut i_step = 0;
        let mut i = 0;
        while i_step != data_slice.len() {
            let i_next = i_step + info.chunk_stride;
            hash_array[i] = hash_data(&data_slice[i_step..i_next]) as HashKey;
            i_step = i_next;
            i += 1;
        }
    } else {
        // fast-path for bytes
        for i in 0..data_slice.len() {
            hash_array[i] = hash_data_single(data_slice[i]) as HashKey;
        }
    }
}

/// Similar to `hash_array_from_data`,
/// but able to step into the next chunk if we run-out of data.
fn hash_array_from_cref(
    info: &BArrayInfo, mut cref: PtrMut<BChunkRef>, data_len: usize,
    hash_array: &mut [HashKey],
) {
    let hash_array_len = data_len / info.chunk_stride;
    let mut i: usize = 0;
    loop {
        let mut i_next: usize = hash_array_len - i;
        let mut data_trim_len = i_next * info.chunk_stride;
        if data_trim_len > cref.link.data.len() {
            data_trim_len = cref.link.data.len();
            i_next = data_trim_len / info.chunk_stride;
        }
        debug_assert!(data_trim_len <= cref.link.data.len());
        hash_array_from_data(
            info, &cref.link.data[0..data_trim_len], &mut hash_array[i..(i + i_next)]);
        i += i_next;
        cref = cref.next;

        if !((i < hash_array_len) && (cref != null_const())) {
            break;
        }
    }

    // If this isn't equal, the caller didn't properly check
    // that there was enough data left in all chunks
    debug_assert!(i == hash_array_len);
}

fn hash_accum(hash_array: &mut [HashKey], hash_array_len: usize, mut iter_steps: usize) {
    // _very_ unlikely, can happen if you select a chunk-size of 1 for example.
    if unlikely!(iter_steps > hash_array_len) {
        iter_steps = hash_array_len;
    }

    let hash_array_search_len: usize = hash_array_len - iter_steps;
    while iter_steps != 0 {
        let hash_offset: usize = iter_steps;
        for i in 0..hash_array_search_len {
            hash_array[i] += (hash_array[i + hash_offset]) * ((hash_array[i] & 0xff) + 1);
        }
        iter_steps -= 1;
    }
}

/// When we only need a single value, can use a small optimization.
/// we can avoid accumulating the tail of the array a little, each iteration.
fn hash_accum_single(hash_array: &mut [HashKey], mut iter_steps: usize) {
    debug_assert!(iter_steps <= hash_array.len());
    if unlikely!(!(iter_steps <= hash_array.len())) {
        // while this shouldn't happen, avoid crashing
        iter_steps = hash_array.len();
    }
    // We can increase this value each step to avoid accumulating quite as much
    // while getting the same results as hash_accum
    let mut iter_steps_sub = iter_steps;

    while iter_steps != 0 {
        let hash_array_search_len: usize = hash_array.len() - iter_steps_sub;
        let hash_offset: usize = iter_steps;
        for i in 0..hash_array_search_len {
            hash_array[i] += (hash_array[i + hash_offset]) * ((hash_array[i] & 0xff) + 1);
        }
        iter_steps -= 1;
        iter_steps_sub += iter_steps;
    }
}

fn key_from_chunk_ref(
    info: &BArrayInfo, cref: PtrMut<BChunkRef>,
    // avoid reallocating each time
    hash_store: &mut [HashKey],
) -> HashKey {
    // fill in a reusable array
    let mut chunk: PtrMut<BChunk> = cref.link;
    debug_assert_ne!(0, (info.accum_read_ahead_bytes * info.chunk_stride));

    if info.accum_read_ahead_bytes <= chunk.data.len() {
        let mut key: HashKey = chunk.key;

        if key != HASH_TABLE_KEY_UNSET {
            // Using key cache!
            // avoids calculating every time
        } else {
            hash_array_from_cref(info, cref, info.accum_read_ahead_bytes, hash_store);
            hash_accum_single(hash_store, info.accum_steps);
            key = hash_store[0];

            // cache the key
            if key == HASH_TABLE_KEY_UNSET {
                key = HASH_TABLE_KEY_FALLBACK;
            }
            chunk.key = key;
        }
        return key;
    } else {
        // corner case - we're too small, calculate the key each time.
        hash_array_from_cref(info, cref, info.accum_read_ahead_bytes, hash_store);
        hash_accum_single(hash_store, info.accum_steps);
        let mut key: HashKey = hash_store[0];

        if unlikely!(key == HASH_TABLE_KEY_UNSET) {
            key = HASH_TABLE_KEY_FALLBACK;
        }
        return key;
    }
}

fn table_lookup(
    info: &BArrayInfo, table: &Vec<PtrMut<BTableRef>>, table_len: usize, i_table_start: usize,
    data: &[u8], data_len: usize, offset: usize, table_hash_array: &Vec<HashKey>,
) -> PtrMut<BChunkRef> {
    let size_left: usize = data_len - offset;
    let key: HashKey = table_hash_array[((offset - i_table_start) / info.chunk_stride)];
    let key_index = (key % (table_len as HashKey)) as usize;
    let mut tref: PtrMut<BTableRef> = table[key_index];
    while tref != null_const() {
        let cref: PtrMut<BChunkRef> = tref.cref;
        if cref.link.key == key {
            let chunk_test: PtrMut<BChunk> = cref.link;
            if chunk_test.data.len() <= size_left {
                if bchunk_data_compare(chunk_test, data, data_len, offset) {
                    // we could remove the chunk from the table, to avoid multiple hits
                    return cref;
                }
            }
        }
        tref = tref.next;
    }
    null_mut()
}

// End Table Lookup
// ----------------

/// []( } )

/// * `data` Data to store in the returned value.
/// * `data_len_original` Length of data in bytes.
/// * `chunk_list_reference` Reuse this list or chunks within it, don't modify its content.
///
/// Note: The caller is responsible for adding the user.
fn bchunk_list_from_data_merge(
    info: &BArrayInfo, bs_mem: &mut BArrayMemory,
    data: &[u8], data_len_original: usize,
    chunk_list_reference: PtrMut<BChunkList>,
) -> PtrMut<BChunkList> {
    debug_assert_chunklist_size!(chunk_list_reference, chunk_list_reference.total_size);

    // -----------------------------------------------------------------------
    // Fast-Path for exact match
    // Check for exact match, if so, return the current list.

    let mut cref_match_first: PtrMut<BChunkRef> = null_mut();

    let mut chunk_list_reference_skip_len: usize = 0;
    let mut chunk_list_reference_skip_bytes: usize = 0;
    let mut i_prev = 0;

    if USE_FASTPATH_CHUNKS_FIRST {
        let mut full_match: bool = true;

        let mut cref: PtrMut<BChunkRef> = chunk_list_reference.chunk_refs.head;
        while i_prev < data_len_original {
            if  cref != null_mut() &&
                bchunk_data_compare(cref.link, data, data_len_original, i_prev)
            {
                cref_match_first = cref;
                chunk_list_reference_skip_len += 1;
                chunk_list_reference_skip_bytes += cref.link.data.len();
                i_prev += cref.link.data.len();
                cref = cref.next;
            } else {
                full_match = false;
                break;
            }
        }

        if full_match {
            if chunk_list_reference.total_size == data_len_original {
                return chunk_list_reference;
            }
        }
    }
    // End Fast-Path (first)
    // ---------------------

    // Copy until we have a mismatch
    let chunk_list: PtrMut<BChunkList> = bchunk_list_new(bs_mem, data_len_original);
    if cref_match_first != null_const() {
        let mut chunk_size_step: usize = 0;
        let mut cref: PtrMut<BChunkRef> = chunk_list_reference.chunk_refs.head;
        loop {
            let chunk: PtrMut<BChunk> = cref.link;
            chunk_size_step += chunk.data.len();
            bchunk_list_append_only(bs_mem, chunk_list, chunk);
            debug_assert_chunklist_size!(chunk_list, chunk_size_step);
            debug_assert_chunklist_data!(chunk_list, data);
            if cref == cref_match_first {
                break;
            } else {
                cref = cref.next;
            }
        }
        // happens when bytes are removed from the end of the array
        if chunk_size_step == data_len_original {
            return chunk_list;
        }

        i_prev = chunk_size_step;
    } else {
        i_prev = 0;
    }

    // ------------------------------------------------------------------------
    // Fast-Path for end chunks
    //
    // Check for trailing chunks

    // In this case use 'chunk_list_reference_last' to define the last index
    // index_match_last = -1

    // warning, from now on don't use len(data)
    // since we want to ignore chunks already matched
    let mut data_len: usize = data_len_original;

    let mut chunk_list_reference_last: PtrMut<BChunkRef> = null_mut();

    if USE_FASTPATH_CHUNKS_LAST {
        if !chunk_list_reference.chunk_refs.is_empty() {
            let mut cref: PtrMut<BChunkRef> = chunk_list_reference.chunk_refs.tail;
            while
                (cref.prev != null_mut()) &&
                (cref != cref_match_first) &&
                (cref.link.data.len() <= data_len - i_prev)
            {
                let chunk_test: PtrMut<BChunk> = cref.link;
                let offset: usize = data_len - chunk_test.data.len();
                if bchunk_data_compare(chunk_test, data, data_len, offset) {
                    data_len = offset;
                    chunk_list_reference_last = cref;
                    chunk_list_reference_skip_len += 1;
                    chunk_list_reference_skip_bytes += cref.link.data.len();
                    cref = cref.prev;
                } else {
                    break;
                }
            }
        }
    }

    // End Fast-Path (last)
    // --------------------

    // -----------------------------------------------------------------------
    // Check for aligned chunks
    //
    // This saves a lot of searching, so use simple heuristics to detect aligned arrays.
    // (may need to tweak exact method).

    let mut use_aligned: bool = false;

    if USE_ALIGN_CHUNKS_TEST {
        if chunk_list.total_size == chunk_list_reference.total_size {
            // if we're already a quarter aligned
            if data_len - i_prev <= chunk_list.total_size / 4 {
                use_aligned = true;
            } else {
                // TODO, walk over chunks and check if some arbitrary amount align
            }
        }
    }

    // End Aligned Chunk Case
    // ----------------------

    if use_aligned {
        // Copy matching chunks, creates using the same 'layout' as the reference
        let mut cref: PtrMut<BChunkRef> = {
            if cref_match_first != null_mut() {
                cref_match_first.next
            } else {
                chunk_list_reference.chunk_refs.head
            }
        };
        while i_prev != data_len {
            let i: usize = i_prev + cref.link.data.len();
            debug_assert!(i != i_prev);

            if (cref != chunk_list_reference_last) &&
                bchunk_data_compare(cref.link, data, data_len, i_prev)
            {
                bchunk_list_append(info, bs_mem, chunk_list, cref.link);
                debug_assert_chunklist_size!(chunk_list, i);
                debug_assert_chunklist_data!(chunk_list, data);
            } else {
                bchunk_list_append_data(info, bs_mem, chunk_list, &data[i_prev..i]);
                debug_assert_chunklist_size!(chunk_list, i);
                debug_assert_chunklist_data!(chunk_list, data);
            }

            cref = cref.next;

            i_prev = i;
        }
    } else if
        (data_len - i_prev >= info.chunk_byte_size) &&
        (chunk_list_reference.chunk_refs_len >= chunk_list_reference_skip_len) &&
        (chunk_list_reference.chunk_refs.head != null_mut())
    {

        // --------------------------------------------------------------------
        // Non-Aligned Chunk De-Duplication

        // only create a table if we have at least one chunk to search
        // otherwise just make a new one.
        //
        // Support re-arranged chunks

        let i_table_start = i_prev;
        let table_hash_array_len: usize = (data_len - i_prev) / info.chunk_stride;
        let mut table_hash_array: Vec<HashKey> = Vec::with_capacity(table_hash_array_len);
        unsafe { table_hash_array.set_len(table_hash_array_len) };

        hash_array_from_data(info, &data[i_prev..data_len], &mut table_hash_array[..]);

        hash_accum(&mut table_hash_array[..], table_hash_array_len, info.accum_steps);

        let chunk_list_reference_remaining_len: usize =
            (chunk_list_reference.chunk_refs_len - chunk_list_reference_skip_len) + 1;
        let mut table_ref_stack: Vec<BTableRef> =
            Vec::with_capacity(chunk_list_reference_remaining_len);

        let table_len = chunk_list_reference_remaining_len * BCHUNK_HASH_TABLE_MUL;
        let mut table: Vec<PtrMut<BTableRef>> = vec![null_mut(); table_len];

        // table_make - inline
        // include one matching chunk, to allow for repeating values
        {
            // all values are filled
            let mut hash_store: Vec<HashKey> = Vec::with_capacity(info.accum_read_ahead_len);
            unsafe { hash_store.set_len(info.accum_read_ahead_len) };

            let mut chunk_list_reference_bytes_remaining: usize =
                chunk_list_reference.total_size - chunk_list_reference_skip_bytes;

            let mut cref: PtrMut<BChunkRef> = {
                if cref_match_first != null_mut() {
                    chunk_list_reference_bytes_remaining += cref_match_first.link.data.len();
                    cref_match_first
                } else {
                    chunk_list_reference.chunk_refs.head
                }
            };

            if USE_PARANOID_CHECKS {
                let mut test_bytes_len: usize = 0;
                let mut cr: PtrMut<BChunkRef> = cref;
                while cr != chunk_list_reference_last {
                    test_bytes_len += cr.link.data.len();
                    cr = cr.next;
                }
                debug_assert!(test_bytes_len == chunk_list_reference_bytes_remaining);
            }

            while
                (cref != chunk_list_reference_last) &&
                (chunk_list_reference_bytes_remaining >= info.accum_read_ahead_bytes)
            {
                let key: HashKey = key_from_chunk_ref(info, cref, &mut hash_store[..]);
                let key_index: usize = (key % table_len as HashKey) as usize;
                let tref_prev: PtrMut<BTableRef> = table[key_index];
                debug_assert!(table_ref_stack.len() < chunk_list_reference_remaining_len);
                table_ref_stack.push(BTableRef { cref: cref, next: tref_prev });
                table[key_index] = PtrMut(table_ref_stack.last_mut().unwrap());

                chunk_list_reference_bytes_remaining -= cref.link.data.len();
                cref = cref.next;
            }

            debug_assert!(table_ref_stack.len() <= chunk_list_reference_remaining_len);

            drop(hash_store);
        }
        // done making the table

        debug_assert!(i_prev <= data_len);
        let mut i = i_prev;
        while i < data_len {
            // Assumes exiting chunk isnt a match!
            let mut cref_found: PtrMut<BChunkRef> = table_lookup(
                info,
                &table, table_len, i_table_start,
                data, data_len, i, &table_hash_array);

            if cref_found != null_const() {
                debug_assert!(i < data_len);
                if i != i_prev {
                    bchunk_list_append_data_n(info, bs_mem, chunk_list, &data[i_prev..i]);
                    i_prev = i;
                    if false && i_prev != 0 { } // quiet warning!
                }

                // now add the reference chunk
                {
                    let chunk_found: PtrMut<BChunk> = cref_found.link;
                    i += chunk_found.data.len();
                    bchunk_list_append(info, bs_mem, chunk_list, chunk_found);
                }
                i_prev = i;
                debug_assert!(i_prev <= data_len);
                debug_assert_chunklist_size!(chunk_list, i_prev);
                debug_assert_chunklist_data!(chunk_list, data);

                // its likely that the next chunk in the list will be a match, so check it!
                while
                    (cref_found.next != null_mut()) &&
                    (cref_found.next != chunk_list_reference_last)
                {
                    cref_found = cref_found.next;
                    let chunk_found: PtrMut<BChunk> = cref_found.link;

                    if bchunk_data_compare(chunk_found, data, data_len, i_prev) {
                        // may be useful to remove table data,
                        // assuming we dont have repeating memory
                        // where it would be useful to re-use chunks.
                        i += chunk_found.data.len();
                        bchunk_list_append(info, bs_mem, chunk_list, chunk_found);
                        // chunk_found may be freed!
                        i_prev = i;
                        debug_assert!(i_prev <= data_len);
                        debug_assert_chunklist_size!(chunk_list, i_prev);
                        debug_assert_chunklist_data!(chunk_list, data);
                    } else {
                        break;
                    }
                }
            } else {
                i = i + info.chunk_stride;
            }
        }

        drop(table_hash_array);
        drop(table);
        drop(table_ref_stack);

        // End Table Lookup
        // ----------------
    }

    debug_assert_chunklist_size!(chunk_list, i_prev);
    debug_assert_chunklist_data!(chunk_list, data);

    // -----------------------------------------------------------------------
    // No Duplicates to copy, write new chunks
    //
    // Trailing chunks, no matches found in table lookup above.
    // Write all new data. */
    if i_prev != data_len {
        bchunk_list_append_data_n(info, bs_mem, chunk_list, &data[i_prev..data_len]);
        i_prev = data_len;
    }

    debug_assert!(i_prev == data_len);

    if USE_FASTPATH_CHUNKS_LAST {
        if chunk_list_reference_last != null_mut() {
            // write chunk_list_reference_last since it hasn't been written yet
            let mut cref: PtrMut<BChunkRef> = chunk_list_reference_last;
            while cref != null_mut() {
                let chunk: PtrMut<BChunk> = cref.link;
                // debug_assert!(bchunk_data_compare(chunk, data, data_len, i_prev));
                i_prev += chunk.data.len();
                // use simple since we assume the references
                // chunks have already been sized correctly.
                bchunk_list_append_only(bs_mem, chunk_list, chunk);
                debug_assert_chunklist_data!(chunk_list, data);
                cref = cref.next;
            }
        }
    }

    debug_assert!(i_prev == data_len_original);

    // check we're the correct size and that we didn't accidentally modify the reference
    debug_assert_chunklist_size!(chunk_list, data_len_original);
    debug_assert_chunklist_size!(chunk_list_reference, chunk_list_reference.total_size);

    debug_assert_chunklist_data!(chunk_list, data);

    return chunk_list;
}
// end private API

/// []( } )

/// # Main Array Storage API
/// []( { )

///
/// Create a new array store, which can store any number of arrays
/// as long as their stride matches.
///
/// * `stride` the `sizeof()` each element,
///
/// Note while a stride of `1` will always work,
/// its less efficient since duplicate chunks of memory will be searched
/// at positions unaligned with the array data.
///
/// * `chunk_count` Number of elements to split each chunk into.
///
///   * A small value increases the ability to de-duplicate chunks,
///     but adds overhead by increasing the number of chunks
///     to look-up when searching for duplicates,
///     as well as some overhead constructing the original
///     array again, with more calls to ``memcpy``.
///   * Larger values reduce the *book keeping* overhead,
///     but increase the chance a small,
///     isolated change will cause a larger amount of data to be duplicated.
///
/// Return a new array store.
///
impl BArrayStore {
    pub fn new(
        stride: usize,
        chunk_count: usize,
    ) -> BArrayStore {
        let accum_steps = BCHUNK_HASH_TABLE_ACCUMULATE_STEPS - 1;
        let accum_read_ahead_len = ((((accum_steps * (accum_steps + 1))) / 2) + 1) as usize;
        let accum_read_ahead_bytes = accum_read_ahead_len * stride;

        BArrayStore {
            info: BArrayInfo {
                chunk_stride: stride,
                // chunk_count: chunk_count, // UNUSED

                chunk_byte_size: chunk_count * stride,
                chunk_byte_size_min: max(1, chunk_count / BCHUNK_SIZE_MIN_DIV) * stride,
                chunk_byte_size_max: (chunk_count * BCHUNK_SIZE_MAX_MUL) * stride,

                accum_steps: accum_steps,
                // Triangle number, identifying now much read-ahead we need:
                // https://en.wikipedia.org/wiki/Triangular_number (+ 1)
                accum_read_ahead_len: accum_read_ahead_len,
                accum_read_ahead_bytes: accum_read_ahead_bytes,
            },
            memory: BArrayMemory {
                state: MemPool::new(),
                chunk_list: MemPool::new(),
                chunk_ref: MemPool::new(),
                // allow iteration to simplify freeing, otherwise its not needed
                // (we could loop over all states as an alternative).
                chunk: MemPool::new(),
            },
            states: ListBase::new(),
        }
    }

    fn free_data(&mut self) {
        // free chunk data
        for mut chunk in self.memory.chunk.iter_mut() {
            unsafe { ::std::ptr::drop_in_place(&mut chunk.data); }
        }
    }

    /// Clear all contents, allowing reuse of `self`.
    pub fn clear(
        &mut self,
    ) {
        self.free_data();

        self.states.clear();

        self.memory.chunk_list.clear();
        self.memory.chunk_ref.clear();
        self.memory.chunk.clear();
    }

    /// # BArrayStore Statistics
    /// []( { )

    /// return the total amount of memory that would be used by getting the arrays for all states.
    pub fn calc_size_expanded_get(
        &self,
    ) -> usize {
        let mut size_accum: usize = 0;
        for state in self.states.iter() {
            size_accum += state.chunk_list.total_size;
        }
        size_accum
    }

    /// return the amount of memory used by all `BChunk.data`
    /// (duplicate chunks are only counted once).
    pub fn calc_size_compacted_get(
        &self,
    ) -> usize {
        let mut size_total: usize = 0;
        for chunk in self.memory.chunk.iter() {
            debug_assert!(chunk.users > 0);
            size_total += chunk.data.len();
        }
        size_total
    }

    /// []( } )

    /// # BArrayState Access
    /// []( { )
    ///
    /// * `data` Data used to create.
    ///
    /// * `state_reference` The state to use as a reference when adding the new state,
    ///   typically this is the previous state,
    ///   however it can be any previously created state from this `self`.
    ///
    /// Returns the new state,
    /// which is used by the caller as a handle to get back the contents of `data`.
    /// This may be removed using `BArrayStore.state_remove`,
    /// otherwise it will be removed with `BArrayStore.destroy`.
    ///
    pub fn state_add(
        &mut self,
        data: &[u8],
        state_reference: Option<*const BArrayState>,
    ) -> *mut BArrayState {
        // ensure we're aligned to the stride
        debug_assert_eq!(0, data.len() % self.info.chunk_stride);

        if USE_PARANOID_CHECKS {
            if let Some(state_reference) = state_reference {
                assert!(self.states.index_at(PtrConst(state_reference)).is_some());
            }
        }

        let mut chunk_list = {
            if let Some(state_reference) = state_reference {
                bchunk_list_from_data_merge(
                    &self.info, &mut self.memory,
                    data, data.len(),
                    // re-use reference chunks
                    PtrConst(state_reference).chunk_list,
                )
            } else {
                let chunk_list = bchunk_list_new(&mut self.memory, data.len());
                bchunk_list_fill_from_array(
                    &self.info, &mut self.memory,
                    chunk_list,
                    data,
                );
                chunk_list
            }
        };

        chunk_list.users += 1;

        let state = PtrMut(self.memory.state.alloc_elem_from(
            BArrayState {
                next: null_mut(),
                prev: null_mut(),
                chunk_list: chunk_list,
            })
        );

        self.states.push_back(state);

        if USE_PARANOID_CHECKS {
            let data_test = BArrayStore::state_data_get_alloc(state.as_ptr());
            assert_eq!(data_test.len(), data.len());
            // we don't want to print the
            assert!(data_test == data);
            // data_test gets freed
        }

        return state.as_ptr();
    }

    /// Remove a state and free any unused `BChunk` data.
    ///
    /// The states can be freed in any order.
    pub fn state_remove(
        &mut self,
        state: *mut BArrayState,
    ) {
        let state = PtrMut(state);
        if USE_PARANOID_CHECKS {
            assert!(self.states.index_at(state).is_some());
        }

        bchunk_list_decref(&mut self.memory, state.chunk_list);
        self.states.remove(state);

        self.memory.state.free_elem(state.as_ptr());
    }

    /// return the expanded size of the array,
    /// use this to know how much memory to allocate `BArrayStore.state_data_get` 's argument.
    pub fn state_size_get(
        state: *const BArrayState,
    ) -> usize {
        return unsafe { (*state).chunk_list.total_size };
    }

    /// Fill in existing allocated memory with the contents of `state`.
    pub fn state_data_get(
        state: *const BArrayState,
        data: &mut [u8],
    ) {
        let state = PtrConst(state);
        if USE_PARANOID_CHECKS {
            let mut data_test_len: usize = 0;
            for cref in state.chunk_list.chunk_refs.iter() {
                data_test_len += cref.link.data.len();
            }
            assert_eq!(data_test_len, state.chunk_list.total_size);
            assert_eq!(data_test_len, data.len());
        }

        debug_assert_eq!(state.chunk_list.total_size, data.len());
        let mut data_step = 0;
        for cref in state.chunk_list.chunk_refs.iter() {
            let data_step_next = data_step + cref.link.data.len();
            debug_assert!(cref.link.users > 0);
            {
                let aaa = &cref.link.data[..];
                data[data_step..data_step_next].clone_from_slice(aaa);
            }
            data_step = data_step_next;
        }
    }

    /// Allocate an array for `state` and return it.
    pub fn state_data_get_alloc(
        state: *const BArrayState,
    ) -> Vec<u8> {
        let state = PtrConst(state);
        let mut data: Vec<u8> = Vec::with_capacity(state.chunk_list.total_size);
        unsafe { data.set_len(state.chunk_list.total_size) };
        BArrayStore::state_data_get(state.as_ptr(), &mut data[..]);
        return data;
    }

    pub fn is_valid(
        &self,
    ) -> bool {

        // Check Length
        // ------------

        for state in self.states.iter() {
            let chunk_list: PtrMut<BChunkList> = state.chunk_list;
            if bchunk_list_size(chunk_list) != chunk_list.total_size {
                return false;
            }

            if chunk_list.chunk_refs.len_calc() != chunk_list.chunk_refs_len {
                return false;
            }

            if USE_MERGE_CHUNKS {
                // ensure we merge all chunks that could be merged
                if chunk_list.total_size > self.info.chunk_byte_size_min {
                    for cref in chunk_list.chunk_refs.iter() {
                        if cref.link.data.len() < self.info.chunk_byte_size_min {
                            return false;
                        }
                    }
                }
            }
        }

        // Check User Count & Lost References
        // ----------------------------------

        {
            use std::collections::HashMap;
            use std::collections::hash_map::Entry::{
                Occupied,
                Vacant,
            };
            macro_rules! GHASH_PTR_ADD_USER {
                ($gh:expr, $pt:expr) => {
                    match $gh.entry($pt.as_ptr()) {
                        Occupied(mut val) => {
                            *val.get_mut() += 1;
                        },
                        Vacant(entry) => {
                            entry.insert(1);
                        }
                    }
                }
            }


            // count chunk_list's
            let mut chunk_list_map: HashMap<*const BChunkList, isize> = HashMap::new();
            let mut chunk_map: HashMap<*const BChunk, isize> = HashMap::new();

            let mut totrefs: usize = 0;
            for state in self.states.iter() {
                GHASH_PTR_ADD_USER!(chunk_list_map, state.chunk_list);
            }
            for (chunk_list, users) in chunk_list_map.iter() {
                if !(unsafe { (**chunk_list).users } == *users) {
                    return false;
                }
            }
            if !(self.memory.chunk_list.len() == chunk_list_map.len()) {
                return false;
            }

            // count chunk's
            for (chunk_list, _users) in chunk_list_map.iter() {
                for cref in unsafe { (**chunk_list) .chunk_refs.iter() } {
                    GHASH_PTR_ADD_USER!(chunk_map, cref.link);
                    totrefs += 1;
                }
            }
            if self.memory.chunk.len() != chunk_map.len() {
                return false;
            }
            if self.memory.chunk_ref.len() != totrefs {
                return false;
            }

            for (chunk, users) in chunk_map.iter() {
                if !(unsafe { (**chunk).users } == *users) {
                    return false;
                }
            }
        }
        return true;
    }

}

impl Drop for BArrayStore {
    fn drop(&mut self) {
        self.free_data();
    }
}

/// # Debugging API (for testing).
/// []( { )

// only for test validation
fn bchunk_list_size(chunk_list: PtrMut<BChunkList>) -> usize {
    let mut total_size: usize = 0;
    for cref in chunk_list.chunk_refs.iter() {
        total_size += cref.link.data.len();
    }
    return total_size;
}

// []( } )
