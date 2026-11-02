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

//! `sha256` against NIST's CAVP short- and long-message vectors, committed
//! with their source in `tests/vectors/sha256/README.md`.

use softshell_core::sha256::sha256;

const SHORT: &str = include_str!("vectors/sha256/SHA256ShortMsg.rsp");
const LONG: &str = include_str!("vectors/sha256/SHA256LongMsg.rsp");

fn from_hex(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0, "odd hex length");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("vector files hold hex"))
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Checks every `Len`, `Msg`, `MD` triple in a CAVP response file and
/// returns how many there were.
fn check(file: &str, name: &str) -> usize {
    let (mut len, mut msg, mut count) = (None, None, 0);
    for line in file.lines().map(str::trim) {
        if let Some(bits) = line.strip_prefix("Len = ") {
            len = Some(bits.parse::<usize>().expect("Len is a decimal bit count"));
        } else if let Some(hex) = line.strip_prefix("Msg = ") {
            msg = Some(from_hex(hex));
        } else if let Some(md) = line.strip_prefix("MD = ") {
            let bits = len.take().expect("Len before MD");
            let msg = msg.take().expect("Msg before MD");
            assert_eq!(bits % 8, 0, "{name}: byte-oriented vectors only");
            // `Len = 0` comes with `Msg = 00`: the length decides.
            let message = &msg[..bits / 8];
            assert_eq!(to_hex(&sha256(message)), md, "{name}, Len = {bits}");
            count += 1;
        }
    }
    count
}

#[test]
fn short_messages() {
    assert_eq!(check(SHORT, "SHA256ShortMsg.rsp"), 65);
}

#[test]
fn long_messages() {
    assert_eq!(check(LONG, "SHA256LongMsg.rsp"), 64);
}
