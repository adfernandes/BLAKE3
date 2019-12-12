use crate::{CVBytes, CVWords, IncrementCounter, BLOCK_LEN, CHUNK_LEN, OUT_LEN};
use arrayref::array_ref;
use arrayvec::ArrayVec;
use core::usize;
use rand::prelude::*;

// Interesting input lengths to run tests on.
pub const TEST_CASES: &[usize] = &[
    0,
    1,
    CHUNK_LEN - 1,
    CHUNK_LEN,
    CHUNK_LEN + 1,
    2 * CHUNK_LEN,
    2 * CHUNK_LEN + 1,
    3 * CHUNK_LEN,
    3 * CHUNK_LEN + 1,
    4 * CHUNK_LEN,
    4 * CHUNK_LEN + 1,
    5 * CHUNK_LEN,
    5 * CHUNK_LEN + 1,
    6 * CHUNK_LEN,
    6 * CHUNK_LEN + 1,
    7 * CHUNK_LEN,
    7 * CHUNK_LEN + 1,
    8 * CHUNK_LEN,
    8 * CHUNK_LEN + 1,
    16 * CHUNK_LEN, // AVX512's bandwidth
    31 * CHUNK_LEN, // 16 + 8 + 4 + 2 + 1
];

pub const TEST_CASES_MAX: usize = 31 * CHUNK_LEN;

// There's a test to make sure these two are equal below.
pub const TEST_KEY: CVBytes = *b"whats the Elvish word for friend";
pub const TEST_KEY_WORDS: CVWords = [
    1952540791, 1752440947, 1816469605, 1752394102, 1919907616, 1868963940, 1919295602, 1684956521,
];

// Paint the input with a repeating byte pattern. We use a cycle length of 251,
// because that's the largets prime number less than 256. This makes it
// unlikely to swapping any two adjacent input blocks or chunks will give the
// same answer.
pub fn paint_test_input(buf: &mut [u8]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
}

type CompressInPlaceFn =
    unsafe fn(cv: &mut CVWords, block: &[u8; BLOCK_LEN], block_len: u8, counter: u64, flags: u8);

type CompressXofFn = unsafe fn(
    cv: &CVWords,
    block: &[u8; BLOCK_LEN],
    block_len: u8,
    counter: u64,
    flags: u8,
) -> [u8; 64];

// A shared helper function for platform-specific tests.
pub fn test_compress_fn(compress_in_place_fn: CompressInPlaceFn, compress_xof_fn: CompressXofFn) {
    let initial_state = TEST_KEY_WORDS;
    let block_len: u8 = 61;
    let mut block = [0; BLOCK_LEN];
    paint_test_input(&mut block[..block_len as usize]);
    // Use a counter with set bits in both 32-bit words.
    let counter = (5u64 << 32) + 6;
    let flags = crate::CHUNK_END | crate::ROOT | crate::KEYED_HASH;

    let portable_out =
        crate::portable::compress_xof(&initial_state, &block, block_len, counter as u64, flags);

    let mut test_state = initial_state;
    unsafe { compress_in_place_fn(&mut test_state, &block, block_len, counter as u64, flags) };
    let test_state_bytes = crate::platform::le_bytes_from_words_32(&test_state);
    let test_xof =
        unsafe { compress_xof_fn(&initial_state, &block, block_len, counter as u64, flags) };

    assert_eq!(&portable_out[..32], &test_state_bytes[..]);
    assert_eq!(&portable_out[..], &test_xof[..]);
}

type HashManyFn<A> = unsafe fn(
    inputs: &[&A],
    key: &CVWords,
    counter: u64,
    increment_counter: IncrementCounter,
    flags: u8,
    flags_start: u8,
    flags_end: u8,
    out: &mut [u8],
);

// A shared helper function for platform-specific tests.
pub fn test_hash_many_fn(
    hash_many_chunks_fn: HashManyFn<[u8; CHUNK_LEN]>,
    hash_many_parents_fn: HashManyFn<[u8; 2 * OUT_LEN]>,
) {
    // 31 (16 + 8 + 4 + 2 + 1) inputs
    const NUM_INPUTS: usize = 31;
    let mut input_buf = [0; CHUNK_LEN * NUM_INPUTS];
    crate::test::paint_test_input(&mut input_buf);
    // A counter just prior to u32::MAX.
    let counter = (1u64 << 32) - 1;

    // First hash chunks.
    let mut chunks = ArrayVec::<[&[u8; CHUNK_LEN]; NUM_INPUTS]>::new();
    for i in 0..NUM_INPUTS {
        chunks.push(array_ref!(input_buf, i * CHUNK_LEN, CHUNK_LEN));
    }
    let mut portable_chunks_out = [0; NUM_INPUTS * OUT_LEN];
    crate::portable::hash_many(
        &chunks,
        &TEST_KEY_WORDS,
        counter,
        IncrementCounter::Yes,
        crate::DERIVE_KEY,
        crate::CHUNK_START,
        crate::CHUNK_END,
        &mut portable_chunks_out,
    );

    let mut test_chunks_out = [0; NUM_INPUTS * OUT_LEN];
    unsafe {
        hash_many_chunks_fn(
            &chunks[..],
            &TEST_KEY_WORDS,
            counter,
            IncrementCounter::Yes,
            crate::DERIVE_KEY,
            crate::CHUNK_START,
            crate::CHUNK_END,
            &mut test_chunks_out,
        );
    }
    for n in 0..NUM_INPUTS {
        #[cfg(feature = "std")]
        dbg!(n);
        assert_eq!(
            &portable_chunks_out[n * OUT_LEN..][..OUT_LEN],
            &test_chunks_out[n * OUT_LEN..][..OUT_LEN]
        );
    }

    // Then hash parents.
    let mut parents = ArrayVec::<[&[u8; 2 * OUT_LEN]; NUM_INPUTS]>::new();
    for i in 0..NUM_INPUTS {
        parents.push(array_ref!(input_buf, i * 2 * OUT_LEN, 2 * OUT_LEN));
    }
    let mut portable_parents_out = [0; NUM_INPUTS * OUT_LEN];
    crate::portable::hash_many(
        &parents,
        &TEST_KEY_WORDS,
        0,
        IncrementCounter::No,
        crate::DERIVE_KEY | crate::PARENT,
        0,
        0,
        &mut portable_parents_out,
    );

    let mut test_parents_out = [0; NUM_INPUTS * OUT_LEN];
    unsafe {
        hash_many_parents_fn(
            &parents[..],
            &TEST_KEY_WORDS,
            0,
            IncrementCounter::No,
            crate::DERIVE_KEY | crate::PARENT,
            0,
            0,
            &mut test_parents_out,
        );
    }
    for n in 0..NUM_INPUTS {
        #[cfg(feature = "std")]
        dbg!(n);
        assert_eq!(
            &portable_parents_out[n * OUT_LEN..][..OUT_LEN],
            &test_parents_out[n * OUT_LEN..][..OUT_LEN]
        );
    }
}

#[test]
fn test_key_bytes_equal_key_words() {
    assert_eq!(
        TEST_KEY_WORDS,
        crate::platform::words_from_le_bytes_32(&TEST_KEY),
    );
}

#[test]
fn test_reference_impl_size() {
    // Because the Rust compiler optimizes struct layout, it's possible that
    // some future version of the compiler will produce a different size. If
    // that happens, we can either disable this test, or test for multiple
    // expected values. For now, the purpose of this test is to make sure we
    // notice if that happens.
    assert_eq!(1848, core::mem::size_of::<reference_impl::Hasher>());
}

#[test]
fn test_counter_words() {
    let counter: u64 = (1 << 32) + 2;
    assert_eq!(crate::counter_low(counter), 2);
    assert_eq!(crate::counter_high(counter), 1);
}

#[test]
fn test_largest_power_of_two_leq() {
    let input_output = &[
        // The zero case is nonsensical, but it does work.
        (0, 1),
        (1, 1),
        (2, 2),
        (3, 2),
        (4, 4),
        (5, 4),
        (6, 4),
        (7, 4),
        (8, 8),
        // the largest possible usize
        (usize::MAX, (usize::MAX >> 1) + 1),
    ];
    for &(input, output) in input_output {
        assert_eq!(
            output,
            crate::largest_power_of_two_leq(input),
            "wrong output for n={}",
            input
        );
    }
}

#[test]
fn test_left_len() {
    let input_output = &[
        (CHUNK_LEN + 1, CHUNK_LEN),
        (2 * CHUNK_LEN - 1, CHUNK_LEN),
        (2 * CHUNK_LEN, CHUNK_LEN),
        (2 * CHUNK_LEN + 1, 2 * CHUNK_LEN),
        (4 * CHUNK_LEN - 1, 2 * CHUNK_LEN),
        (4 * CHUNK_LEN, 2 * CHUNK_LEN),
        (4 * CHUNK_LEN + 1, 4 * CHUNK_LEN),
    ];
    for &(input, output) in input_output {
        assert_eq!(crate::left_len(input), output);
    }
}

#[test]
fn test_compare_reference_impl() {
    const OUT: usize = 303; // more than 64, not a multiple of 4
    let mut input_buf = [0; TEST_CASES_MAX];
    paint_test_input(&mut input_buf);
    for &case in TEST_CASES {
        let input = &input_buf[..case];
        #[cfg(feature = "std")]
        dbg!(case);

        // regular
        {
            let mut reference_hasher = reference_impl::Hasher::new();
            reference_hasher.update(input);
            let mut expected_out = [0; OUT];
            reference_hasher.finalize(&mut expected_out);

            let test_out = crate::hash(input);
            assert_eq!(&test_out, array_ref!(expected_out, 0, 32));
            let mut hasher = crate::Hasher::new();
            hasher.update(input);
            assert_eq!(&hasher.finalize(), array_ref!(expected_out, 0, 32));
            assert_eq!(&hasher.finalize(), &test_out);
            let mut extended = [0; OUT];
            hasher.finalize_xof().fill(&mut extended);
            assert_eq!(&extended[..], &expected_out[..]);
        }

        // keyed
        {
            let mut reference_hasher = reference_impl::Hasher::new_keyed(&TEST_KEY);
            reference_hasher.update(input);
            let mut expected_out = [0; OUT];
            reference_hasher.finalize(&mut expected_out);

            let test_out = crate::keyed_hash(&TEST_KEY, input);
            assert_eq!(&test_out, array_ref!(expected_out, 0, 32));
            let mut hasher = crate::Hasher::new_keyed(&TEST_KEY);
            hasher.update(input);
            assert_eq!(&hasher.finalize(), array_ref!(expected_out, 0, 32));
            assert_eq!(&hasher.finalize(), &test_out);
            let mut extended = [0; OUT];
            hasher.finalize_xof().fill(&mut extended);
            assert_eq!(&extended[..], &expected_out[..]);
        }

        // derive_key
        {
            let mut reference_hasher = reference_impl::Hasher::new_derive_key(&TEST_KEY);
            reference_hasher.update(input);
            let mut expected_out = [0; OUT];
            reference_hasher.finalize(&mut expected_out);

            let test_out = crate::derive_key(&TEST_KEY, input);
            assert_eq!(&test_out, array_ref!(expected_out, 0, 32));
            let mut hasher = crate::Hasher::new_derive_key(&TEST_KEY);
            hasher.update(input);
            assert_eq!(&hasher.finalize(), array_ref!(expected_out, 0, 32));
            assert_eq!(&hasher.finalize(), &test_out);
            let mut extended = [0; OUT];
            hasher.finalize_xof().fill(&mut extended);
            assert_eq!(&extended[..], &expected_out[..]);
        }
    }
}

fn reference_hash(input: &[u8]) -> crate::Hash {
    let mut hasher = reference_impl::Hasher::new();
    hasher.update(input);
    let mut bytes = [0; 32];
    hasher.finalize(&mut bytes);
    bytes.into()
}

#[test]
fn test_compare_update_multiple() {
    // Don't use all the long test cases here, since that's unnecessarily slow
    // in debug mode.
    let short_test_cases = &TEST_CASES[..10];
    assert_eq!(*short_test_cases.last().unwrap(), 4 * CHUNK_LEN);

    let mut input_buf = [0; 2 * TEST_CASES_MAX];
    paint_test_input(&mut input_buf);

    for &first_update in short_test_cases {
        #[cfg(feature = "std")]
        dbg!(first_update);
        let first_input = &input_buf[..first_update];
        let mut test_hasher = crate::Hasher::new();
        test_hasher.update(first_input);

        for &second_update in short_test_cases {
            #[cfg(feature = "std")]
            dbg!(second_update);
            let second_input = &input_buf[first_update..][..second_update];
            let total_input = &input_buf[..first_update + second_update];
            // Clone the hasher with first_update bytes already written, so
            // that the next iteration can reuse it.
            let mut test_hasher = test_hasher.clone();
            test_hasher.update(second_input);

            assert_eq!(reference_hash(total_input), test_hasher.finalize());
        }
    }
}

#[test]
fn test_fuzz_hasher() {
    const INPUT_MAX: usize = 4 * CHUNK_LEN;
    let mut input_buf = [0; 3 * INPUT_MAX];
    paint_test_input(&mut input_buf);

    // Don't do too many iterations in debug mode, to keep the tests under a
    // second or so. CI should run tests in release mode also. Provide an
    // environment variable for specifying a larger number of fuzz iterations.
    let mut num_tests: usize = if cfg!(debug_assertions) { 100 } else { 10_000 };
    #[cfg(feature = "std")]
    {
        if let Ok(iters) = std::env::var("BLAKE3_FUZZ_ITERATIONS") {
            num_tests = iters.parse().expect("invalid usize");
        }
    }

    // Use a fixed RNG seed for reproducibility.
    let mut rng = rand_chacha::ChaCha8Rng::from_seed([1; 32]);
    for num_test in 0..num_tests {
        #[cfg(feature = "std")]
        dbg!(num_test);
        let mut hasher = crate::Hasher::new();
        let mut total_input = 0;
        // For each test, write 3 inputs of random length.
        for _ in 0..3 {
            let input_len = rng.gen_range(0, INPUT_MAX + 1);
            #[cfg(feature = "std")]
            dbg!(input_len);
            let input = &input_buf[total_input..][..input_len];
            hasher.update(input);
            total_input += input_len;
        }
        let expected = reference_hash(&input_buf[..total_input]);
        assert_eq!(expected, hasher.finalize());
    }
}

#[test]
fn test_xof_seek() {
    let mut out = [0; 533];
    let mut hasher = crate::Hasher::new();
    hasher.update(b"foo");
    hasher.finalize_xof().fill(&mut out);
    assert_eq!(hasher.finalize().as_bytes(), &out[0..32]);

    let mut reader = hasher.finalize_xof();
    reader.set_position(303);
    let mut out2 = [0; 102];
    reader.fill(&mut out2);
    assert_eq!(&out[303..][..102], &out2[..]);

    #[cfg(feature = "std")]
    {
        use std::io::prelude::*;
        let mut reader = hasher.finalize_xof();
        reader.seek(std::io::SeekFrom::Start(303)).unwrap();
        let mut out3 = Vec::new();
        reader.by_ref().take(102).read_to_end(&mut out3).unwrap();
        assert_eq!(&out[303..][..102], &out3[..]);

        assert_eq!(
            reader.seek(std::io::SeekFrom::Current(0)).unwrap(),
            303 + 102
        );
        reader.seek(std::io::SeekFrom::Current(-5)).unwrap();
        assert_eq!(
            reader.seek(std::io::SeekFrom::Current(0)).unwrap(),
            303 + 102 - 5
        );
        let mut out4 = [0; 17];
        assert_eq!(reader.read(&mut out4).unwrap(), 17);
        assert_eq!(&out[303 + 102 - 5..][..17], &out4[..]);
        assert_eq!(
            reader.seek(std::io::SeekFrom::Current(0)).unwrap(),
            303 + 102 - 5 + 17
        );
        assert!(reader.seek(std::io::SeekFrom::End(0)).is_err());
        assert!(reader.seek(std::io::SeekFrom::Current(-1000)).is_err());
    }
}
