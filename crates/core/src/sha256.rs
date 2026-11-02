// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! SHA-256, as FIPS 180-4 specifies it.
//!
//! It is used for one thing: storing API tokens as hashes (ADR-0019). Any
//! other use of SHA-256, or of any other cryptographic primitive, goes
//! through the dependency policy's exception test first.
//!
//! ```
//! use softshell_core::sha256::sha256;
//!
//! assert_eq!(
//!     sha256(b"abc")[..4],
//!     [0xBA, 0x78, 0x16, 0xBF],
//! );
//! ```

/// The initial hash value (FIPS 180-4, 5.3.3): the first 32 bits of the
/// fractional parts of the square roots of the first 8 primes.
const H0: [u32; 8] = [
    0x6A09_E667,
    0xBB67_AE85,
    0x3C6E_F372,
    0xA54F_F53A,
    0x510E_527F,
    0x9B05_688C,
    0x1F83_D9AB,
    0x5BE0_CD19,
];

/// The round constants (FIPS 180-4, 4.2.2): the first 32 bits of the
/// fractional parts of the cube roots of the first 64 primes.
const K: [u32; 64] = [
    0x428A_2F98,
    0x7137_4491,
    0xB5C0_FBCF,
    0xE9B5_DBA5,
    0x3956_C25B,
    0x59F1_11F1,
    0x923F_82A4,
    0xAB1C_5ED5,
    0xD807_AA98,
    0x1283_5B01,
    0x2431_85BE,
    0x550C_7DC3,
    0x72BE_5D74,
    0x80DE_B1FE,
    0x9BDC_06A7,
    0xC19B_F174,
    0xE49B_69C1,
    0xEFBE_4786,
    0x0FC1_9DC6,
    0x240C_A1CC,
    0x2DE9_2C6F,
    0x4A74_84AA,
    0x5CB0_A9DC,
    0x76F9_88DA,
    0x983E_5152,
    0xA831_C66D,
    0xB003_27C8,
    0xBF59_7FC7,
    0xC6E0_0BF3,
    0xD5A7_9147,
    0x06CA_6351,
    0x1429_2967,
    0x27B7_0A85,
    0x2E1B_2138,
    0x4D2C_6DFC,
    0x5338_0D13,
    0x650A_7354,
    0x766A_0ABB,
    0x81C2_C92E,
    0x9272_2C85,
    0xA2BF_E8A1,
    0xA81A_664B,
    0xC24B_8B70,
    0xC76C_51A3,
    0xD192_E819,
    0xD699_0624,
    0xF40E_3585,
    0x106A_A070,
    0x19A4_C116,
    0x1E37_6C08,
    0x2748_774C,
    0x34B0_BCB5,
    0x391C_0CB3,
    0x4ED8_AA4A,
    0x5B9C_CA4F,
    0x682E_6FF3,
    0x748F_82EE,
    0x78A5_636F,
    0x84C8_7814,
    0x8CC7_0208,
    0x90BE_FFFA,
    0xA450_6CEB,
    0xBEF9_A3F7,
    0xC671_78F2,
];

/// The SHA-256 digest of `data`.
#[must_use]
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state = H0;
    let (blocks, rest) = data.as_chunks::<64>();
    for block in blocks {
        compress(&mut state, block);
    }

    // Padding (FIPS 180-4, 5.1.1): the leftover bytes, 0x80, zeros, and the
    // message length in bits as a big-endian u64, in one block or, when
    // fewer than 9 bytes are free, two.
    let mut tail = [0u8; 128];
    for (dst, src) in tail.iter_mut().zip(rest) {
        *dst = *src;
    }
    // `rest` holds at most 63 bytes, so these always find their bytes.
    if let Some(marker) = tail.get_mut(rest.len()) {
        *marker = 0x80;
    }
    let tail_len = if rest.len() < 56 { 64 } else { 128 };
    // A slice holds at most isize::MAX bytes. Its length in bits overflows
    // u64 only at 2^61 bytes or more: outside FIPS 180-4's domain, and more
    // than any supported machine can hold.
    let bits = (data.len() as u64).wrapping_mul(8);
    if let Some(field) = tail.get_mut(tail_len - 8..tail_len) {
        field.copy_from_slice(&bits.to_be_bytes());
    }
    let (tail_blocks, _) = tail.as_chunks::<64>();
    for block in tail_blocks.iter().take(tail_len / 64) {
        compress(&mut state, block);
    }

    let mut digest = [0u8; 32];
    let (out, _) = digest.as_chunks_mut::<4>();
    for (bytes, word) in out.iter_mut().zip(state) {
        *bytes = word.to_be_bytes();
    }
    digest
}

/// Processes one 64-byte block (FIPS 180-4, 6.2.2).
fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    let (words, _) = block.as_chunks::<4>();
    for (wt, bytes) in w.iter_mut().zip(words) {
        *wt = u32::from_be_bytes(*bytes);
    }
    for t in 16..64 {
        w[t] = small_sigma1(w[t - 2])
            .wrapping_add(w[t - 7])
            .wrapping_add(small_sigma0(w[t - 15]))
            .wrapping_add(w[t - 16]);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for (&k, &wt) in K.iter().zip(&w) {
        let t1 = h
            .wrapping_add(big_sigma1(e))
            .wrapping_add(ch(e, f, g))
            .wrapping_add(k)
            .wrapping_add(wt);
        let t2 = big_sigma0(a).wrapping_add(maj(a, b, c));
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *s = s.wrapping_add(v);
    }
}

const fn ch(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (!x & z)
}

const fn maj(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (x & z) ^ (y & z)
}

const fn big_sigma0(x: u32) -> u32 {
    x.rotate_right(2) ^ x.rotate_right(13) ^ x.rotate_right(22)
}

const fn big_sigma1(x: u32) -> u32 {
    x.rotate_right(6) ^ x.rotate_right(11) ^ x.rotate_right(25)
}

const fn small_sigma0(x: u32) -> u32 {
    x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
}

const fn small_sigma1(x: u32) -> u32 {
    x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Byte `i` of the shared test pattern.
    fn pattern(len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| u8::try_from((i * 31 + 7) % 256).unwrap())
            .collect()
    }

    /// The examples published with the standard (FIPS 180-2, appendix B,
    /// and the empty message).
    #[test]
    fn standard_examples() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            hex(&sha256(&vec![b'a'; 1_000_000])),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    /// Every length 0 to 256, covering every padding case (one or two final
    /// blocks, and each byte position of the 0x80 marker), against digests
    /// computed independently.
    #[test]
    fn every_length_through_four_blocks() {
        let mut digests = Vec::new();
        for len in 0..=256 {
            digests.extend_from_slice(&sha256(&pattern(len)));
        }
        assert_eq!(
            hex(&sha256(&digests)),
            "740197f2831c5c43d18de5a75028010e654a0d3886d9af3808294bfd1b460bde"
        );
    }

    /// A 2 MiB message: its length in bits, 2^24, reaches the fourth byte
    /// of the length field, which shorter messages leave zero.
    #[test]
    fn length_field_beyond_three_bytes() {
        assert_eq!(
            hex(&sha256(&pattern(1 << 21))),
            "15d80ba308304e391a817479e20a3922787105dfdff12e2251e83d80f8c32195"
        );
    }

    /// The integer `n`th root of `x`, rounded down.
    fn root(x: u128, n: u32) -> u128 {
        let (mut lo, mut hi) = (0u128, 1u128 << (128 / n + 1));
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if mid.checked_pow(n).is_some_and(|p| p <= x) {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo
    }

    /// The constants, recomputed from their definitions: the first 32 bits
    /// of the fractional part of the root of a prime p are the low 32 bits
    /// of the integer root of p * 2^(32n).
    #[test]
    fn constants_match_their_definitions() {
        let primes: Vec<u128> = (2u128..)
            .filter(|&n| (2..n).take_while(|d| d * d <= n).all(|d| n % d != 0))
            .take(64)
            .collect();
        for (i, &p) in primes.iter().enumerate() {
            let cube = root(p << 96, 3) as u32;
            assert_eq!(K[i], cube, "K[{i}] (prime {p})");
            if let Some(&h) = H0.get(i) {
                assert_eq!(h, root(p << 64, 2) as u32, "H0[{i}] (prime {p})");
            }
        }
    }
}
