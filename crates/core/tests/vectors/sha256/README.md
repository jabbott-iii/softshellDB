# NIST CAVP SHA-256 test vectors

`softshell_core::sha256` must produce every digest in these files
(roadmap task M0.3; test: `crates/core/tests/sha256_cavp.rs`).

## Source

- **Publisher:** National Institute of Standards and Technology (NIST),
  Cryptographic Algorithm Validation Program (CAVP), "SHA Test Vectors for
  Hashing Byte-Oriented Messages".
- **Archive:** `shabytetestvectors.zip`, 4,909,729 bytes, SHA-256
  `929ef80b7b3418aca026643f6f248815913b60e01741a44bba9e118067f4c9b8`, from
  <https://csrc.nist.gov/CSRC/media/Projects/Cryptographic-Algorithm-Validation-Program/documents/shs/shabytetestvectors.zip>
  (server `Last-Modified`: 2024-09-29).
- **Retrieved:** 2026-11-02.
- **Kept:** only these two members of the archive, byte for byte, CRLF line
  endings included. Their own header reads `CAVS 11.0`, generated
  2011-03-15.

| File | Bytes | SHA-256 | Vectors |
| :- | -: | :- | :- |
| `SHA256ShortMsg.rsp` | 10,299 | `75e1cb83994638481808e225b9eb0c1ebd0c232d952ac42b61abce6363be283c` | 65 (0 to 512 bits) |
| `SHA256LongMsg.rsp` | 426,209 | `6fac36f37360bcf74ffcf4465c18e30d6d5a04cc90885b901fc3130c16060974` | 64 (1,304 to 51,200 bits) |

Before they were committed, every vector was checked against Python's
`hashlib`; all 129 matched.

## Terms

These vectors were produced by NIST, an agency of the United States
government. As a work of the U.S. federal government they are not subject
to copyright protection in the United States (17 U.S.C. § 105). They are
test data published with the FIPS 180-4 standard, not code, and are kept
here unmodified with this notice (`docs/02-dependency-policy.md`,
ADR-0004).

`.gitattributes` marks this directory `-text`, so Git never rewrites the
line endings and the checksums above stay valid on every OS.
