// Licensed: Apache 2.0

extern crate block_array_cow;

mod rand;

use block_array_cow::{
    BArrayStore,
    BArrayState,
};

use ::std::ptr::{
    null_mut,
};

const DEBUG_PRINT: bool = false;

fn print_mem_saved(id: &str, bs: &BArrayStore) {
    let size_real   = bs.calc_size_compacted_get();
    let size_expand = bs.calc_size_expanded_get();
    let percent = {
        if size_expand != 0 {
            (size_real as f64 / size_expand as f64) * 100.0
        } else {
            -1.0
        }
    };
    println!("{}: {:.8}%", id, percent);
}

// const WORDS: &'static [u8] = include_bytes!("words_small.in");
const WORDS: &'static [u8] = include_bytes!("words10k.in");


// --------------------------------------------------------------------
// Test Chunks (building data from list of chunks)

struct TestChunk {
    data: Vec<u8>,
}

fn testchunk_list_add(
    cl: &mut Vec<TestChunk>, data: Vec<u8>,
) {
    cl.push(TestChunk { data: data });
}

fn testchunk_list_free(cl: &mut Vec<TestChunk>) {
    cl.clear();
}

fn testchunk_as_data_array(
    tc_array: &Vec<TestChunk>,
) -> Vec<u8> {
    let mut data_len: usize = 0;
    for c in tc_array {
        data_len += c.data.len();
    }
    let mut data: Vec<u8> = Vec::with_capacity(data_len);
    for c in tc_array {
        data.extend_from_slice(&c.data[..]);
    }
    return data;
}


// --------------------------------------------------------------------
// Test Buffer

// API to handle local allocation of data so we can compare it with the data in the array_store
struct TestBuffer {
    data: Vec<u8>,
    // for reference
    state: *mut BArrayState,
}

fn testbuffer_list_add(cl: &mut Vec<TestBuffer>, data: Vec<u8>) {
    cl.push(TestBuffer { data: data, state: null_mut() });
}

fn testbuffer_list_add_copydata(cl: &mut Vec<TestBuffer>, data: &[u8]) {
    let mut data_copy: Vec<u8> = Vec::with_capacity(data.len());
    data_copy.extend_from_slice(data);
    testbuffer_list_add(cl, data_copy);
}

fn testbuffer_list_state_from_data(cl: &mut Vec<TestBuffer>, data: &[u8]) {
    testbuffer_list_add_copydata(cl, data);
}


// A version of testbuffer_list_state_from_data that expand data by stride,
// handy so we can test data at different strides.
fn testbuffer_list_state_from_data_ex_stride_expand(
    cl: &mut Vec<TestBuffer>,
    data: &[u8],
    stride: usize,
) {
    if stride == 1 {
        testbuffer_list_state_from_data(cl, data);
    } else {
        let data_stride_len = data.len() * stride;
        let mut data_stride: Vec<u8> = Vec::with_capacity(data_stride_len);

        for c in data {
            for _ in 0..stride {
                data_stride.push(*c);
            }
        }

        testbuffer_list_add(cl, data_stride);
    }
}

macro_rules! testbuffer_list_state_from_string_array {
    ($lb:expr, $data_array:expr) => {
        {
            let data_array: Vec<&'static [u8]> = $data_array;
            for data in data_array {
                testbuffer_list_state_from_data($lb, &data[..]);
            }
        }
    }
}

macro_rules! testbuffer_strings_create {
    ($lb:expr, $strings:expr) => {
        $lb.clear();
        testbuffer_list_state_from_string_array!($lb, $strings);
    }
}

// test in both directions
macro_rules! testbuffer_strings_ex {
    ($bs:expr, $strings:expr) => {
        let mut cl: Vec<TestBuffer> = Vec::new();
        testbuffer_strings_create!(&cl, strings);

        testbuffer_run_tests(bs, &cl);

        testbuffer_list_free(&cl);
    }
}


macro_rules! testbuffer_strings {
    ($stride:expr, $chunk_count:expr, $strings:expr) => {
        let mut cl: Vec<TestBuffer> = Vec::new();
        testbuffer_strings_create!(&mut cl, $strings);

        testbuffer_run_tests_simple(&mut cl, $stride, $chunk_count);

        testbuffer_list_free(&mut cl);
    }
}

fn testbuffer_item_validate(tb: &TestBuffer) -> bool {
    let mut ok = true;
    let data_state = BArrayStore::state_data_get_alloc(tb.state);
    if tb.data.len() != data_state.len() {
        ok = false;
    } else if &data_state[..] != &tb.data[..] {
        ok = false;
    }
    drop(data_state);
    return ok;
}

fn testbuffer_list_validate(
    cl: &mut Vec<TestBuffer>,
) -> bool {
    for tb in cl {
        if !testbuffer_item_validate(tb) {
            return false;
        }
    }
    return true;
}

fn testbuffer_list_data_randomize(cl: &mut Vec<TestBuffer>, mut random_seed: u32) {
    let mut rng = rand::Rng::new(0);
    for tb in cl {
        rng.seed(random_seed);
        rng.shuffle(&mut tb.data[..]);
        random_seed += 1;
    }
}

fn testbuffer_list_store_populate(
    bs: &mut BArrayStore, cl: &mut Vec<TestBuffer>,
) {
    let mut state_prev: Option<*const BArrayState> = None;
    for mut tb in cl {
        tb.state = bs.state_add(&tb.data[..], state_prev);
        state_prev = Some(tb.state);
    }
}

fn testbuffer_list_store_clear(
    bs: &mut BArrayStore, cl: &mut Vec<TestBuffer>,
) {
    for mut tb in cl {
        bs.state_remove(tb.state);
        tb.state = null_mut();
    }
}

fn testbuffer_list_free(
    cl: &mut Vec<TestBuffer>,
) {
    cl.clear();
}

fn testbuffer_run_tests_single(
    bs: &mut BArrayStore, cl: &mut Vec<TestBuffer>,
) {
    testbuffer_list_store_populate(bs, cl);
    assert!(testbuffer_list_validate(cl));
    assert!(bs.is_valid());
    if DEBUG_PRINT {
        print_mem_saved("data", bs);
    }
}

// avoid copy-paste code to run tests
fn testbuffer_run_tests(
    bs: &mut BArrayStore, cl: &mut Vec<TestBuffer>,
) {
    // forwards
    testbuffer_run_tests_single(bs, cl);
    testbuffer_list_store_clear(bs, cl);

    cl.reverse();

    // backwards
    testbuffer_run_tests_single(bs, cl);
    testbuffer_list_store_clear(bs, cl);
}

fn testbuffer_run_tests_simple(
    cl: &mut Vec<TestBuffer>,
    stride: usize, chunk_count: usize,
) {
    let mut bs: BArrayStore = BArrayStore::new(stride, chunk_count);
    testbuffer_run_tests(&mut bs, cl);
    bs.clear();

}


// --------------------------------------------------------------------
// Basic Tests

#[test]
fn nop() {
    let mut bs = BArrayStore::new(1, 32);
    bs.clear();
}

#[test]
fn nop_state() {
    let mut bs = BArrayStore::new(1, 32);
    let data = b"test";
    let state = bs.state_add(data, None);
    assert_eq!(4, BArrayStore::state_size_get(state));
    bs.state_remove(state);
    bs.clear();
}

#[test]
fn single() {
    let mut bs = BArrayStore::new(1, 32);
    let data_src = b"test";
    let state = bs.state_add(data_src, None);
    let data_dst = BArrayStore::state_data_get_alloc(state);
    assert_eq!(data_src.len(), data_dst.len());
    assert_eq!(data_src, &data_dst[..]);
}

#[test]
fn double_nop() {
    let mut bs = BArrayStore::new(1, 32);
    let data_src = b"test";

    let state_a = bs.state_add(data_src, None);
    let state_b = bs.state_add(data_src, Some(state_a));

    assert_eq!(bs.calc_size_compacted_get(), data_src.len());
    assert_eq!(bs.calc_size_expanded_get(), data_src.len() * 2);

    let mut data_dst;
    data_dst = BArrayStore::state_data_get_alloc(state_a);
    assert_eq!(data_src, &data_dst[..]);

    data_dst = BArrayStore::state_data_get_alloc(state_b);
    assert_eq!(data_src, &data_dst[..]);
}

#[test]
fn double_diff() {
    let mut bs = BArrayStore::new(1, 32);
    let data_src_a = b"test";
    let data_src_b = b"####";

    let state_a = bs.state_add(data_src_a, None);
    let state_b = bs.state_add(data_src_b, Some(state_a));

    assert_eq!(bs.calc_size_compacted_get(), data_src_a.len() * 2);
    assert_eq!(bs.calc_size_expanded_get(), data_src_a.len() * 2);

    let mut data_dst;
    data_dst = BArrayStore::state_data_get_alloc(state_a);
    assert_eq!(data_src_a, &data_dst[..]);

    data_dst = BArrayStore::state_data_get_alloc(state_b);
    assert_eq!(data_src_b, &data_dst[..]);
}

#[test]
fn text_mixed() {
    testbuffer_strings!(1, 4, vec![b""]);
    testbuffer_strings!(1, 4, vec![b"test"]);
    testbuffer_strings!(1, 4, vec![b"", b"test"]);
    testbuffer_strings!(1, 4, vec![b"test", b""]);
    testbuffer_strings!(1, 4, vec![b"test", b"", b"test"]);
    testbuffer_strings!(1, 4, vec![b"", b"test", b""]);
}

#[test]
fn text_dupe_increase_decrease() {
    let mut cl: Vec<TestBuffer> = Vec::new();

    macro_rules! expand {
        ($lb:expr, $d:expr) => {
            testbuffer_strings_create!(
                $lb,
                vec![
                    $d.as_bytes(),
                    concat!($d, $d).as_bytes(),
                    concat!($d, $d, $d).as_bytes(),
                    concat!($d, $d, $d, $d).as_bytes(),
                ]
            );
        }
    }
    let chunk_size = 8;
    expand!(&mut cl, "#1#2#3#4");

    let mut bs = BArrayStore::new(1, chunk_size);

    // forward
    testbuffer_list_store_populate(&mut bs, &mut cl);
    assert!(testbuffer_list_validate(&mut cl));
    assert!(bs.is_valid());
    assert_eq!(bs.calc_size_compacted_get(), chunk_size);

    testbuffer_list_store_clear(&mut bs, &mut cl);
    cl.reverse();

    // backwards
    testbuffer_list_store_populate(&mut bs, &mut cl);
    assert!(testbuffer_list_validate(&mut cl));
    assert!(bs.is_valid());
    // larger since first block doesn't de-duplicate
    assert_eq!(bs.calc_size_compacted_get(), chunk_size * 4);

    testbuffer_list_free(&mut cl);
}


// --------------------------------------------------------------------
// Plain Text Tests

/// Test that uses text input with different params for the array-store
/// to ensure no corner cases fail.
fn plain_text_helper(
    words: &[u8], word_delim: u8,
    stride: usize, chunk_count: usize, random_seed: u32)
{
    let mut cl: Vec<TestBuffer> = Vec::new();

    let mut i_prev = 0;
    for i in 0..words.len() {
        if words[i] == word_delim {
            if i != i_prev {
                testbuffer_list_state_from_data_ex_stride_expand(
                    &mut cl, &words[i_prev..i], stride);
            }
            i_prev = i;
        }
    }
    if i_prev + 1 != words.len() {
        testbuffer_list_state_from_data_ex_stride_expand(
            &mut cl, &words[i_prev..], stride);
    }

    if random_seed != 0 {
        testbuffer_list_data_randomize(&mut cl, random_seed);
    }

    testbuffer_run_tests_simple(&mut cl, stride, chunk_count);

    testbuffer_list_free(&mut cl);
}

// split by '.' (multiple words)
#[test] fn text_sentences_chunk_1()    { plain_text_helper(WORDS, b'.', 1,    1, 0); }
#[test] fn text_sentences_chunk_2()    { plain_text_helper(WORDS, b'.', 1,    2, 0); }
#[test] fn text_sentences_chunk_8()    { plain_text_helper(WORDS, b'.', 1,    8, 0); }
#[test] fn text_sentences_chunk_32()   { plain_text_helper(WORDS, b'.', 1,   32, 0); }
#[test] fn text_sentences_chunk_128()  { plain_text_helper(WORDS, b'.', 1,  128, 0); }
#[test] fn text_sentences_chunk_1024() { plain_text_helper(WORDS, b'.', 1, 1024, 0); }
// odd numbers
#[test] fn text_sentences_chunk_3()   { plain_text_helper(WORDS, b'.', 1,   3, 0); }
#[test] fn text_sentences_chunk_13()  { plain_text_helper(WORDS, b'.', 1,  13, 0); }
#[test] fn text_sentences_chunk_131() { plain_text_helper(WORDS, b'.', 1, 131, 0); }

// split by ' ', individual words
#[test] fn text_words_chunk_1()    { plain_text_helper(WORDS, b' ', 1,    1, 0); }
#[test] fn text_words_chunk_2()    { plain_text_helper(WORDS, b' ', 1,    2, 0); }
#[test] fn text_words_chunk_8()    { plain_text_helper(WORDS, b' ', 1,    8, 0); }
#[test] fn text_words_chunk_32()   { plain_text_helper(WORDS, b' ', 1,   32, 0); }
#[test] fn text_words_chunk_128()  { plain_text_helper(WORDS, b' ', 1,  128, 0); }
#[test] fn text_words_chunk_1024() { plain_text_helper(WORDS, b' ', 1, 1024, 0); }
// odd numbers
#[test] fn text_words_chunk_3()   { plain_text_helper(WORDS, b' ', 1,   3, 0); }
#[test] fn text_words_chunk_13()  { plain_text_helper(WORDS, b' ', 1,  13, 0); }
#[test] fn text_words_chunk_131() { plain_text_helper(WORDS, b' ', 1, 131, 0); }

// various tests with different strides & randomizing
#[test] fn text_sentences_random_stride3_chunk3()    { plain_text_helper(WORDS, b'q',   3,   3, 7337); }
#[test] fn text_sentences_random_stride8_chunk8()    { plain_text_helper(WORDS, b'n',   8,   8, 5667); }
#[test] fn text_sentences_random_stride32_chunk1()   { plain_text_helper(WORDS, b'a',   1,  32, 1212); }
#[test] fn text_sentences_random_stride12_chunk512() { plain_text_helper(WORDS, b'g',  12, 512, 9999); }
#[test] fn text_sentences_random_stride128_chunk6()  { plain_text_helper(WORDS, b'b',  20,   6, 1000); }


/* -------------------------------------------------------------------- */
/* Random Data Tests */

fn rand_range_i(rng: &mut rand::Rng, min_i: usize, max_i: usize, step: usize) -> usize {
    if min_i == max_i {
        return min_i;
    }
    debug_assert!(min_i <= max_i);
    debug_assert!(((min_i % step) == 0) && ((max_i % step) == 0));
    let range: usize = max_i - min_i;
    min_i + (((rng.get::<usize>() % range) / step) * step)
}

/**
 * In-place array wrap.
 * (rotate the array one step forward or backwards).
 *
 * Access via #BLI_array_wrap
 */
fn array_wrap(arr: &mut [u8], arr_len: usize, arr_stride: usize, reverse: bool) {
    use ::std::ptr;
    let arr_sub = arr_len - 1;
    let mut buf: Vec<u8> = Vec::with_capacity(arr_stride);
    unsafe { buf.set_len(arr_stride); }

    unsafe {
        if !reverse {
            ptr::copy_nonoverlapping(&arr[0], &mut buf[0], arr_stride);
            ptr::copy(&arr[arr_stride], &mut arr[0], arr_stride * arr_sub);
            ptr::copy_nonoverlapping(&buf[0], &mut arr[arr_stride * arr_sub], arr_stride);
        } else {
            ptr::copy_nonoverlapping(&arr[arr_stride * arr_sub], &mut buf[0], arr_stride);
            ptr::copy(&arr[0], &mut arr[arr_stride], arr_stride * arr_sub);
            ptr::copy_nonoverlapping(&buf[0], &mut arr[0], arr_stride);
        }
    }
}

fn testbuffer_list_state_random_data(
    cl: &mut Vec<TestBuffer>,
    stride: usize,
    data_min_len: usize, data_max_len: usize,
    mutate: usize, rng: &mut rand::Rng,
) {
    use ::std::ptr;
    let mut data_len: usize = rand_range_i(rng, data_min_len, data_max_len + stride, stride);
    let mut data: Vec<u8> = Vec::with_capacity(data_len);
    unsafe { data.set_len(data_len); }
    if cl.is_empty() {
        rng.fill(&mut data[..]);
    } else {
        let tb_last = cl.last_mut().unwrap();
        if tb_last.data.len() >= data_len {
            data.clone_from_slice(&tb_last.data[0..data_len]);
        } else {
            data[0..tb_last.data.len()].clone_from_slice(&tb_last.data[..]);
            rng.fill(&mut data[tb_last.data.len()..]);
        }
        // perform multiple small mutations to the array.
        for _ in 0..mutate {
            const MUTATE_NOP: u32 = 0;
            const MUTATE_ADD: u32 = 1;
            const MUTATE_REMOVE: u32 = 2;
            const MUTATE_ROTATE: u32 = 3;
            const MUTATE_RANDOMIZE: u32 = 4;
            const MUTATE_TOTAL: u32 = 5;
            // mutate
            match rng.get::<u32>() % MUTATE_TOTAL {
                MUTATE_NOP => {},
                MUTATE_ADD => {
                    let offset: usize = rand_range_i(rng, 0, data_len, stride);
                    if data_len < data_max_len {
                        data_len += stride;
                        data.reserve(stride);
                        unsafe {
                            data.set_len(data_len);
                        }
                        if offset + stride < data_len {
                            unsafe {
                                ptr::copy(
                                    &data[offset],
                                    &mut data[offset + stride],
                                    data_len - (offset + stride),
                                );
                            }
                        }
                        rng.fill(&mut data[offset..(offset + stride)]);
                    }
                },
                MUTATE_REMOVE => {
                    let offset: usize = rand_range_i(rng, 0, data_len, stride);
                    if data_len > data_min_len {
                        if data_len > offset + stride {
                            unsafe {
                                ptr::copy(
                                    &data[offset + stride],
                                    &mut data[offset],
                                    data_len - (offset + stride),
                                );
                            }
                        }
                        data_len -= stride;
                        data.truncate(data_len);
                    }
                },
                MUTATE_ROTATE => {
                    let items: usize = data_len / stride;
                    if items > 1 {
                        array_wrap(&mut data[..], items, stride, (rng.get::<u32>() % 2) != 0);
                    }
                },
                MUTATE_RANDOMIZE => {
                    if data_len > 0 {
                        let offset: usize = rand_range_i(rng, 0, data_len - stride, stride);
                        rng.fill(&mut data[offset..(offset + stride)]);
                    }
                },
                _ => {
                    // panic!();
                }
            }
        }
    }
    testbuffer_list_add(cl, data);
}

fn random_data_mutate_helper(
    items_size_min: usize, items_size_max: usize, items_total: usize,
    stride: usize, chunk_count: usize,
    random_seed: u32, mutate: usize)
{
    let mut cl: Vec<TestBuffer> = Vec::new();

    let data_min_len = items_size_min * stride;
    let data_max_len = items_size_max * stride;

    {
        let mut rng = rand::Rng::new(random_seed);
        for _ in 0..items_total {
            testbuffer_list_state_random_data(
                &mut cl, stride, data_min_len, data_max_len, mutate, &mut rng);
        }
    }

    testbuffer_run_tests_simple(&mut cl, stride, chunk_count);
}

#[test] fn rand_data_stride1_chunk32_mutate2()  { random_data_mutate_helper(0,   100,  400,  1,  32,  9779, 2); }
#[test] fn rand_data_stride8_chunk512_mutate2() { random_data_mutate_helper(0,   128,  400,  8, 512,  1001, 2); }
#[test] fn rand_data_stride12_chunk48_mutate2() { random_data_mutate_helper(200, 256,  400, 12,  48,  1331, 2); }
#[test] fn rand_data_stride32_chunk64_mutate1() { random_data_mutate_helper(0,   256,  200, 32,  64,  3112, 1); }
#[test] fn rand_data_stride32_chunk64_mutate8() { random_data_mutate_helper(0,   256,  200, 32,  64,  7117, 8); }


/* -------------------------------------------------------------------- */
/* Randomized Chunks Test */

fn random_chunk_generate(
    cl: &mut Vec<TestChunk>,
    chunks_per_buffer: usize,
    stride: usize, chunk_count: usize,
    random_seed: u32)
{
    let mut rng = rand::Rng::new(random_seed);
    let chunk_size_bytes: usize = stride * chunk_count;
    for _ in 0..chunks_per_buffer {
        let mut data_chunk: Vec<u8> = Vec::with_capacity(chunk_size_bytes);
        unsafe { data_chunk.set_len(chunk_size_bytes); }
        rng.fill(&mut data_chunk);
        testchunk_list_add(cl, data_chunk);
    }
}

/**
 * Add random chunks, then re-order them to ensure chunk de-duplication is working.
 */
fn random_chunk_mutate_helper(
    chunks_per_buffer: usize, items_total: usize,
    stride: usize, chunk_count: usize,
    random_seed: u32)
{
    // generate random chunks
    let mut chunks_array: Vec<TestChunk> = Vec::with_capacity(chunks_per_buffer);
    random_chunk_generate(&mut chunks_array, chunks_per_buffer, stride, chunk_count, random_seed);

    // add and re-order each time
    let mut cl: Vec<TestBuffer> = Vec::with_capacity(items_total);

    {
        let mut rng = rand::Rng::new(random_seed);
        for _ in 0..items_total {
            rng.shuffle(&mut chunks_array[..]);
            let data = testchunk_as_data_array(&chunks_array);
            assert_eq!(data.len(), chunks_per_buffer * chunk_count * stride);
            testbuffer_list_add(&mut cl, data);
        }
    }

    testchunk_list_free(&mut chunks_array);
    drop(chunks_array);

    let mut bs = BArrayStore::new(stride, chunk_count);

	testbuffer_run_tests_single(&mut bs, &mut cl);

	let expected_size: usize = chunks_per_buffer * chunk_count * stride;
	assert_eq!(bs.calc_size_compacted_get(), expected_size);

    drop(bs);

	testbuffer_list_free(&mut cl);
}

#[test] fn rand_chunk_8_stride1_chunk64()   { random_chunk_mutate_helper(8,  100,  1, 64, 9779); }
#[test] fn rand_chunk_32_stride1_chunk64()  { random_chunk_mutate_helper(32, 100,  1, 64, 1331); }
#[test] fn rand_chunk_64_stride8_chunk32()  { random_chunk_mutate_helper(64, 100,  8, 32, 2772); }
#[test] fn rand_chunk_31_stride11_chunk21() { random_chunk_mutate_helper(31, 100, 11, 21, 7117); }

