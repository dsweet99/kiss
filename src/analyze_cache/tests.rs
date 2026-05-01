use super::fnv1a64;

#[test]
fn fnv1a64_properties() {
    let seed = 0xcbf2_9ce4_8422_2325_u64;
    assert_eq!(fnv1a64(seed, b""), seed);
    assert_eq!(fnv1a64(seed, b"hello"), fnv1a64(seed, b"hello"));
    assert_ne!(fnv1a64(seed, b"hello"), fnv1a64(seed, b"world"));
}
