//! Bijective 32-bit mixing functions. Each step is invertible, so the
//! composition is a bijection over u32.
//!
//! See: <https://github.com/skeeto/hash-prospector?tab=readme-ov-file#three-round-functions>

#[cfg(test)]
const fn mod_inverse(a: u32) -> u32 {
    let mut x: u32 = 1;
    let mut i = 0;

    while i < 5 {
        x = x.wrapping_mul(2u32.wrapping_sub(a.wrapping_mul(x)));
        i += 1;
    }

    x
}

#[cfg(test)]
#[inline]
fn unmix(mut x: u32, seed: u32) -> u32 {
    const INV_1: u32 = mod_inverse(0x31848bab);
    const INV_2: u32 = mod_inverse(0xac4c1b51);
    const INV_3: u32 = mod_inverse(0xed5ad4bb);

    x ^= x >> 14;
    x ^= x >> 28;
    x = x.wrapping_mul(INV_1);
    x ^= x >> 15;
    x ^= x >> 30;
    x = x.wrapping_mul(INV_2);
    x ^= x >> 11;
    x ^= x >> 22;
    x = x.wrapping_mul(INV_3);
    x ^= x >> 17;
    x ^= seed;
    x
}

#[inline]
fn mix(mut x: u32, seed: u32) -> u32 {
    x ^= seed;
    x ^= x >> 17;
    x = x.wrapping_mul(0xed5ad4bb);
    x ^= x >> 11;
    x = x.wrapping_mul(0xac4c1b51);
    x ^= x >> 15;
    x = x.wrapping_mul(0x31848bab);
    x ^= x >> 14;
    x
}

/// Unmaps a sequential counter to a random-looking identifier. Bijective and
/// 0-preserving: unmap(0, seed) == 0 for any seed.
#[cfg(test)]
pub(crate) fn unmap(id: u32, seed: u32) -> u32 {
    unmix(id ^ mix(0, seed), seed)
}

/// Maps a sequential counter to a random-looking identifier. Bijective and
/// 0-preserving: map(0, seed) == 0 for any seed.
pub(crate) fn map(count: u32, seed: u32) -> u32 {
    mix(count, seed) ^ mix(0, seed)
}

#[cfg(test)]
mod tests {
    const SEEDS: &[u32] = &[0x32b21703, 0xdeadbeef, 0xcafebabe, 0, u32::MAX];

    use super::{map, unmap};

    #[test]
    #[cfg(all(not(miri), not(debug_assertions)))]
    fn test_mix_full() {
        for i in 1..u32::MAX {
            let id = map(i, SEEDS[0]);
            assert_eq!(unmap(id, SEEDS[0]), i);
        }
    }

    #[test]
    fn test_mix_short() {
        for &seed in SEEDS {
            assert_eq!(map(0, seed), 0);
            assert_eq!(unmap(0, seed), 0);

            for i in (1..128).chain([u32::MAX / 2 - 1, u32::MAX]) {
                let id = map(i, seed);
                assert_eq!(unmap(id, seed), i);
            }
        }
    }
}
