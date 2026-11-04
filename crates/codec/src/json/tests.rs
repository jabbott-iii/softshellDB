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

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::error::Error as _;

use softshell_core::civil::CivilError;
use softshell_core::clock::Timestamp;
use softshell_core::error::Chain;
use softshell_core::hash::fnv1a64;
use softshell_core::id::{Id, ParseIdError};
use softshell_core::limits::Limits;
use softshell_core::rng::Pcg32;
use softshell_core::testing::{check, check_sized};

use super::*;
use crate::base64::Base64Error;
use crate::jsonc;
use crate::test_values;
use crate::value::{Object, value};

use JsonErrorKind as K;

fn json(text: &str) -> Value {
    parse(text, Limits::default()).unwrap_or_else(|e| panic!("{text:?}: {e}"))
}

fn object(entries: Vec<(&str, Value)>) -> Value {
    let entries: Vec<(String, Value)> = entries
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
    Value::Object(Object::try_from(entries).unwrap())
}

fn assert_identical(found: &Value, expected: &Value, context: &str) {
    assert!(
        found.identical(expected),
        "{context}: found {found:?}, expected {expected:?}"
    );
}

/// The error for `text`, checked to be `kind` at `line` and `column`.
fn assert_error(dialect: Dialect, text: &str, kind: &JsonErrorKind, line: usize, column: usize) {
    let result = match dialect {
        Dialect::Json => parse(text, Limits::default()),
        Dialect::Jsonc => jsonc::parse(text, Limits::default()),
    };
    let err = result.expect_err(text);
    assert_eq!(
        (err.kind(), err.line(), err.column()),
        (kind, line, column),
        "{text:?}: {err}"
    );
}

#[test]
fn rfc_8259_examples_parse() {
    // RFC 8259 §13.
    let image = r#"{
        "Image": {
            "Width":  800,
            "Height": 600,
            "Title":  "View from 15th Floor",
            "Thumbnail": {
                "Url":    "http://www.example.com/image/481989943",
                "Height": 125,
                "Width":  100
            },
            "Animated" : false,
            "IDs": [116, 943, 234, 38793]
          }
      }"#;
    let expected = value!({
        "Image": {
            "Width": 800,
            "Height": 600,
            "Title": "View from 15th Floor",
            "Thumbnail": {
                "Url": "http://www.example.com/image/481989943",
                "Height": 125,
                "Width": 100
            },
            "Animated": false,
            "IDs": [116, 943, 234, 38793]
        }
    });
    assert_identical(&json(image), &expected, "image");

    let places = r#"[
        {
           "precision": "zip",
           "Latitude":  37.7668,
           "Longitude": -122.3959,
           "Address":   "",
           "City":      "SAN FRANCISCO",
           "State":     "CA",
           "Zip":       "94107",
           "Country":   "US"
        },
        {
           "precision": "zip",
           "Latitude":  37.371991,
           "Longitude": -122.026020,
           "Address":   "",
           "City":      "SUNNYVALE",
           "State":     "CA",
           "Zip":       "94085",
           "Country":   "US"
        }
      ]"#;
    let expected = value!([
        {
            "precision": "zip", "Latitude": 37.7668, "Longitude": -122.3959, "Address": "",
            "City": "SAN FRANCISCO", "State": "CA", "Zip": "94107", "Country": "US"
        },
        {
            "precision": "zip", "Latitude": 37.371991, "Longitude": -122.026020, "Address": "",
            "City": "SUNNYVALE", "State": "CA", "Zip": "94085", "Country": "US"
        }
    ]);
    assert_identical(&json(places), &expected, "places");

    assert_identical(
        &json("\"Hello world!\""),
        &Value::from("Hello world!"),
        "string",
    );
    assert_identical(&json("42"), &Value::Int(42), "number");
    assert_identical(&json("true"), &Value::Bool(true), "literal");
}

#[test]
fn accepts_rfc_8259() {
    let cases: Vec<(&str, Value)> = vec![
        (" \t\n\r1\r\n ", Value::Int(1)),
        ("0", Value::Int(0)),
        ("-0", Value::Int(0)),
        ("-0.0", Value::Float(-0.0)),
        ("0.0", Value::Float(0.0)),
        ("9223372036854775807", Value::Int(i64::MAX)),
        ("-9223372036854775808", Value::Int(i64::MIN)),
        (
            "9223372036854775808",
            Value::Float(9_223_372_036_854_775_808.0),
        ),
        (
            "-9223372036854775809",
            Value::Float(-9_223_372_036_854_775_808.0),
        ),
        ("1E2", Value::Float(100.0)),
        ("1e-2", Value::Float(0.01)),
        ("1.5e+3", Value::Float(1500.0)),
        ("1e-400", Value::Float(0.0)),
        ("-1e-400", Value::Float(-0.0)),
        ("1.7976931348623157e308", Value::Float(f64::MAX)),
        (
            "123456789012345678901234567890",
            Value::Float(1.234_567_890_123_456_8e29),
        ),
        ("true", Value::Bool(true)),
        ("false", Value::Bool(false)),
        ("null", Value::Null),
        (r#""""#, Value::from("")),
        (
            r#""\"\\\/\b\f\n\r\t""#,
            Value::from("\"\\/\u{8}\u{c}\n\r\t"),
        ),
        (
            r#""\u0041\u00e9\u20AC\uD83D\uDE00\u0000\udbff\udfff""#,
            Value::from("Aé€😀\0\u{10ffff}"),
        ),
        (
            "\"\u{7f}\u{2028}é😀\u{feff}\"",
            Value::from("\u{7f}\u{2028}é😀\u{feff}"),
        ),
        ("[]", Value::Array(vec![])),
        ("[ ]", Value::Array(vec![])),
        ("{}", Value::Object(Object::new())),
        ("{ }", Value::Object(Object::new())),
        ("[[]]", Value::Array(vec![Value::Array(vec![])])),
        (r#"{"":0}"#, object(vec![("", Value::Int(0))])),
        (
            r#"{"a":1,"b":{"a":2}}"#,
            object(vec![
                ("a", Value::Int(1)),
                ("b", object(vec![("a", Value::Int(2))])),
            ]),
        ),
        // Only the five extended JSON keys are special, and only exactly.
        (
            r#"{"$in":[1]}"#,
            object(vec![("$in", Value::Array(vec![Value::Int(1)]))]),
        ),
        (r#"{"$ID":"x"}"#, object(vec![("$ID", Value::from("x"))])),
    ];
    for (text, expected) in &cases {
        assert_identical(&json(text), expected, text);
        let jsonc = jsonc::parse(text, Limits::default()).unwrap();
        assert_identical(&jsonc, expected, text);
    }
}

#[test]
fn rejects_what_rfc_8259_does_not_allow() {
    let unexpected = |found, expected| K::Unexpected { found, expected };
    let end = |expected| K::UnexpectedEnd { expected };
    let cases: Vec<(&str, JsonErrorKind, usize, usize)> = vec![
        ("", end("a value"), 1, 1),
        ("   ", end("a value"), 1, 4),
        ("\u{feff}1", K::ByteOrderMark, 1, 1),
        ("nul", end("`null`"), 1, 4),
        ("nulL", unexpected('L', "`null`"), 1, 4),
        ("tru e", unexpected(' ', "`true`"), 1, 4),
        ("True", unexpected('T', "a value"), 1, 1),
        ("01", K::LeadingZero, 1, 1),
        ("-01", K::LeadingZero, 1, 1),
        ("-", end("a digit"), 1, 2),
        ("-a", unexpected('a', "a digit"), 1, 2),
        ("1.", end("a digit after `.`"), 1, 3),
        ("1.e5", unexpected('e', "a digit after `.`"), 1, 3),
        (".5", unexpected('.', "a value"), 1, 1),
        ("+1", unexpected('+', "a value"), 1, 1),
        ("1e", end("a digit in the exponent"), 1, 3),
        ("1e+", end("a digit in the exponent"), 1, 4),
        ("1ee5", unexpected('e', "a digit in the exponent"), 1, 3),
        ("NaN", unexpected('N', "a value"), 1, 1),
        ("-Infinity", unexpected('I', "a digit"), 1, 2),
        ("1e309", K::NumberOutOfRange { ty: "f64" }, 1, 1),
        ("[-1e400]", K::NumberOutOfRange { ty: "f64" }, 1, 2),
        ("0x10", unexpected('x', "the end of the text"), 1, 2),
        ("1 2", unexpected('2', "the end of the text"), 1, 3),
        ("[1 2]", unexpected('2', "`,` or `]`"), 1, 4),
        ("[1,]", K::TrailingComma, 1, 3),
        ("[,]", unexpected(',', "a value"), 1, 2),
        ("[1,,2]", unexpected(',', "a value"), 1, 4),
        ("[", end("a value"), 1, 2),
        ("[1", end("`,` or `]`"), 1, 3),
        ("]", unexpected(']', "a value"), 1, 1),
        ("{", end("a string key or `}`"), 1, 2),
        ("{a:1}", unexpected('a', "a string key or `}`"), 1, 2),
        ("{'a':1}", unexpected('\'', "a string key or `}`"), 1, 2),
        (r#"{"a" 1}"#, unexpected('1', "`:` after the key"), 1, 6),
        (r#"{"a":}"#, unexpected('}', "a value"), 1, 6),
        (r#"{"a":1,}"#, K::TrailingComma, 1, 7),
        (r#"{"a":1 "b":2}"#, unexpected('"', "`,` or `}`"), 1, 8),
        (r#"{"a":1,2}"#, unexpected('2', "a string key"), 1, 8),
        (r#"{"a":1"#, end("`,` or `}`"), 1, 7),
        ("\"abc", end("`\"` to end the string"), 1, 5),
        ("\"a\u{1}\"", K::ControlCharacter { found: '\u{1}' }, 1, 3),
        ("\"a\nb\"", K::ControlCharacter { found: '\n' }, 1, 3),
        ("\"\u{1f}\"", K::ControlCharacter { found: '\u{1f}' }, 1, 2),
        ("\"\u{0}\"", K::ControlCharacter { found: '\0' }, 1, 2),
        // Whitespace is space, tab, `\n` and `\r` only.
        ("\u{c}1", unexpected('\u{c}', "a value"), 1, 1),
        ("1\u{b}", unexpected('\u{b}', "the end of the text"), 1, 2),
        ("\u{a0}1", unexpected('\u{a0}', "a value"), 1, 1),
        ("[1,\u{c}2]", unexpected('\u{c}', "a value"), 1, 4),
        ("\"\\x\"", K::InvalidEscape { found: 'x' }, 1, 2),
        ("\"\\é\"", K::InvalidEscape { found: 'é' }, 1, 2),
        ("\"\\", end("an escape character"), 1, 3),
        ("\"\\u12\"", unexpected('"', "a hex digit"), 1, 6),
        ("\"\\u12G4\"", unexpected('G', "a hex digit"), 1, 6),
        ("\"\\uD800\"", K::LoneSurrogate { code: 0xD800 }, 1, 2),
        ("\"\\uDC00\"", K::LoneSurrogate { code: 0xDC00 }, 1, 2),
        (
            "\"\\udbff\\u0041\"",
            K::LoneSurrogate { code: 0xDBFF },
            1,
            2,
        ),
        ("\"x\\uD800x\"", K::LoneSurrogate { code: 0xD800 }, 1, 3),
        ("\"\\u+FFF\"", unexpected('+', "a hex digit"), 1, 4),
        ("\"\\u-FFF\"", unexpected('-', "a hex digit"), 1, 4),
        ("\"\\u 123\"", unexpected(' ', "a hex digit"), 1, 4),
        ("\"\\u12é\"", unexpected('é', "a hex digit"), 1, 6),
        ("5.", end("a digit after `.`"), 1, 3),
        ("inf", unexpected('i', "a value"), 1, 1),
        ("// c\n1", K::Comment, 1, 1),
        ("1 /* c */", K::Comment, 1, 3),
        ("1 /", unexpected('/', "the end of the text"), 1, 3),
        // Lines and columns: `\n`, `\r\n` and a lone `\r` end a line;
        // columns count characters.
        ("[1]\n\n  x", unexpected('x', "the end of the text"), 3, 3),
        ("[1,\r\n 2,]", K::TrailingComma, 2, 3),
        ("\r\r[", end("a value"), 3, 2),
        ("1\rx", unexpected('x', "the end of the text"), 2, 1),
        ("[1,\r", end("a value"), 2, 1),
        ("\"é😀\" x", unexpected('x', "the end of the text"), 1, 6),
    ];
    for (text, kind, line, column) in &cases {
        assert_error(Dialect::Json, text, kind, *line, *column);
    }
}

#[test]
fn jsonc_allows_comments_and_trailing_commas() {
    let cases: Vec<(&str, Value)> = vec![
        (
            "/*a*/{/*b*/\"k\"/*c*/:/*d*/1/*e*/,/*f*/}//g",
            object(vec![("k", Value::Int(1))]),
        ),
        ("[1, // one\r 2 // two\n]", value!([1, 2])),
        ("[1,]", value!([1])),
        ("[1 , ]", value!([1])),
        ("{\"a\":1,}", object(vec![("a", Value::Int(1))])),
        ("/**/1", Value::Int(1)),
        ("/***/1/*/ */", Value::Int(1)),
        ("1// no line break at the end", Value::Int(1)),
        (r#"["//", "/*"]"#, value!(["//", "/*"])),
        (
            "{\"$id\": \"01ARZ3NDEKTSV4RRFFQ69G5FAV\",}",
            Value::Id("01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap()),
        ),
        (
            "{\"$f32vec\": [1, {\"$float\": \"NaN\",},]}",
            Value::F32Vec(vec![1.0, f32::NAN]),
        ),
    ];
    for (text, expected) in &cases {
        let v = jsonc::parse(text, Limits::default()).unwrap_or_else(|e| panic!("{text:?}: {e}"));
        assert_identical(&v, expected, text);
    }
    let unexpected = |found, expected| K::Unexpected { found, expected };
    let errors: Vec<(&str, JsonErrorKind, usize, usize)> = vec![
        ("[,]", unexpected(',', "a value"), 1, 2),
        ("[1,,]", unexpected(',', "a value"), 1, 4),
        ("{,}", unexpected(',', "a string key or `}`"), 1, 2),
        ("{\"a\":1,,}", unexpected(',', "a string key"), 1, 8),
        ("/* never closed", K::UnterminatedComment, 1, 1),
        ("[1,\n /*/ ]", K::UnterminatedComment, 2, 2),
        ("1 /", unexpected('/', "the end of the text"), 1, 3),
        (
            "// only a comment",
            K::UnexpectedEnd {
                expected: "a value",
            },
            1,
            18,
        ),
        (
            "{\"$id\": \"01ARZ3NDEKTSV4RRFFQ69G5FAV\",,}",
            unexpected(',', "`}`"),
            1,
            38,
        ),
    ];
    for (text, kind, line, column) in &errors {
        assert_error(Dialect::Jsonc, text, kind, *line, *column);
    }
    let err = jsonc::parse("[1 2]", Limits::default()).unwrap_err();
    assert_eq!(err.format(), "JSONC");
    assert_eq!(
        err.to_string(),
        "JSONC at line 1, column 4: expected `,` or `]`, found '2'"
    );
}

/// `[` or `{"a":` repeated `n` times, then the closers.
fn nested(n: usize, object: bool) -> String {
    if object {
        format!("{}1{}", r#"{"a":"#.repeat(n), "}".repeat(n))
    } else {
        format!("{}1{}", "[".repeat(n), "]".repeat(n))
    }
}

#[test]
fn depth_is_limited() {
    let limits = Limits::default();
    assert_eq!(limits.max_depth, 64);
    for is_object in [false, true] {
        let ok = nested(64, is_object);
        parse(&ok, limits).unwrap();
        let err = parse(&nested(65, is_object), limits).unwrap_err();
        let column = if is_object { 64 * 5 + 1 } else { 65 };
        assert_eq!(
            (err.kind(), err.line(), err.column()),
            (&K::TooDeep { max: 64 }, 1, column)
        );
    }
    // Extended JSON forms are scalars: they add no depth.
    let mut shallow = Limits::default();
    shallow.max_depth = 1;
    parse(
        r#"[{"$id": "01ARZ3NDEKTSV4RRFFQ69G5FAV"}, {"$f32vec": [1]}]"#,
        shallow,
    )
    .unwrap();
    for text in ["[{}]", "[[]]", r#"[{"a": 1}]"#] {
        let err = parse(text, shallow).unwrap_err();
        assert_eq!(
            (err.kind(), err.column()),
            (&K::TooDeep { max: 1 }, 2),
            "{text}"
        );
    }
    let mut flat = Limits::default();
    flat.max_depth = 0;
    parse("1", flat).unwrap();
    parse(r#"{"$float": "NaN"}"#, flat).unwrap();
    for text in ["[]", "{}"] {
        let err = parse(text, flat).unwrap_err();
        assert_eq!(
            (err.kind(), err.column()),
            (&K::TooDeep { max: 0 }, 1),
            "{text}"
        );
    }
    // A deep text fails at the limit, long before the stack is at risk.
    let err = parse(&"[".repeat(1_000_000), limits).unwrap_err();
    assert_eq!(err.kind(), &K::TooDeep { max: 64 });
}

#[test]
fn sizes_are_limited() {
    let mut limits = Limits::default();
    limits.max_entries = 2;
    parse("[1,2]", limits).unwrap();
    parse(r#"{"a":1,"b":2}"#, limits).unwrap();
    let err = parse("[1,2,3]", limits).unwrap_err();
    assert_eq!(
        (err.kind(), err.column()),
        (&K::TooManyEntries { max: 2 }, 6)
    );
    let err = parse(r#"{"a":1,"b":2,"c":3}"#, limits).unwrap_err();
    assert_eq!(
        (err.kind(), err.column()),
        (&K::TooManyEntries { max: 2 }, 14)
    );
    limits.max_entries = 0;
    parse("[]", limits).unwrap();
    assert_eq!(
        parse("[0]", limits).unwrap_err().kind(),
        &K::TooManyEntries { max: 0 }
    );
    assert_eq!(
        parse(r#"{"a":0}"#, limits).unwrap_err().kind(),
        &K::TooManyEntries { max: 0 }
    );

    let mut limits = Limits::default();
    limits.max_string_bytes = 3;
    parse(r#""abc""#, limits).unwrap();
    parse(r#""\u00e9a""#, limits).unwrap();
    for (text, column) in [
        (r#""abcd""#, 1),
        (r#""ab\u00e9""#, 1),
        (r#""a\n\n\n""#, 1),
        // A run after an escape counts what is already there.
        (r#""\nabc""#, 1),
        (r#"{"\nabc":1}"#, 2),
        (r#"{"abcd":1}"#, 2),
        (r#"[{"$id": "01ARZ3NDEKTSV4RRFFQ69G5FAV"}]"#, 10),
    ] {
        let err = parse(text, limits).unwrap_err();
        assert_eq!(
            (err.kind(), err.column()),
            (&K::StringTooLong { max: 3 }, column),
            "{text}"
        );
    }

    let mut limits = Limits::default();
    limits.max_document_bytes = 4;
    parse("1234", limits).unwrap();
    let err = parse("12345", limits).unwrap_err();
    assert_eq!(
        (err.kind(), err.offset(), err.line(), err.column()),
        (&K::TooLarge { max: 4, found: 5 }, 0, 1, 1)
    );

    let mut limits = Limits::default();
    limits.max_vector_dims = 2;
    parse(r#"{"$f32vec":[1,2]}"#, limits).unwrap();
    let err = parse(r#"{"$f32vec":[1,2,3]}"#, limits).unwrap_err();
    assert_eq!(
        (err.kind(), err.column()),
        (&K::TooManyDimensions { max: 2 }, 17)
    );
}

#[test]
fn duplicate_keys_are_errors() {
    let key = |k: &str| K::DuplicateKey { key: k.to_owned() };
    assert_error(Dialect::Json, r#"{"a":1,"a":2}"#, &key("a"), 1, 8);
    assert_error(Dialect::Json, r#"{"a":1,"\u0061":2}"#, &key("a"), 1, 8);
    assert_error(
        Dialect::Jsonc,
        "{\"a\":1,\n\"b\":2,\n\"a\":3,}",
        &key("a"),
        3,
        1,
    );
    assert_error(Dialect::Json, r#"[{"x":{"y":1,"y":1}}]"#, &key("y"), 1, 14);
    // A repeat inside a nested object is found when that object closes,
    // before one earlier in the text in the object around it.
    assert_error(
        Dialect::Json,
        r#"{"a":1,"a":2,"b":{"c":1,"c":2}}"#,
        &key("c"),
        1,
        25,
    );
    // After an object closes, its keys' offsets are gone too.
    assert_error(
        Dialect::Json,
        r#"[{"x":1},{"a":1,"a":2}]"#,
        &key("a"),
        1,
        17,
    );
    assert_error(
        Dialect::Json,
        r#"{"p":{"x":1},"a":1,"a":2}"#,
        &key("a"),
        1,
        20,
    );
    // Large objects are sorted to find repeats; the first repeat in the
    // text is reported.
    let mut text = String::from("{");
    for i in 0..20 {
        text.push_str(&format!("\"k{i}\":{i},"));
    }
    let column = text.chars().count() + 1;
    text.push_str("\"k5\":0,\"k3\":0}");
    assert_error(Dialect::Json, &text, &key("k5"), 1, column);
    // Keys in different objects may repeat.
    json(r#"{"a":{"a":{"a":1}},"b":[{"a":1},{"a":2}]}"#);

    // The key is in the kind but not the message, which is safe to log.
    let long = "x".repeat(100);
    let err = parse(&format!("{{\"{long}\":1,\"{long}\":2}}"), Limits::default()).unwrap_err();
    assert_eq!(err.kind(), &key(&long));
    assert_eq!(
        err.to_string(),
        "JSON at line 1, column 107: a key repeats an earlier key of the same object"
    );
}

#[test]
fn extended_json_reads_every_form() {
    let id: Id = "01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap();
    let cases: Vec<(&str, Value)> = vec![
        (r#"{"$id": "01ARZ3NDEKTSV4RRFFQ69G5FAV"}"#, Value::Id(id)),
        (r#"{"$id": "01arz3ndektsv4rrffq69g5fav"}"#, Value::Id(id)),
        (
            r#"{"$ts": "2026-11-01T12:00:00.000000001Z"}"#,
            Value::Timestamp(Timestamp::from_nanos(1_793_534_400_000_000_001)),
        ),
        (
            r#"{"$ts": "2026-11-01T13:00:00.5+01:00"}"#,
            Value::Timestamp(Timestamp::from_nanos(1_793_534_400_500_000_000)),
        ),
        (
            r#"{"$ts":"1677-09-21T00:12:43.145224192Z"}"#,
            Value::Timestamp(Timestamp::MIN),
        ),
        (
            r#"{"$ts":"2262-04-11T23:47:16.854775807Z"}"#,
            Value::Timestamp(Timestamp::MAX),
        ),
        (r#"{"$bytes": "AQID"}"#, Value::Bytes(vec![1, 2, 3])),
        // A key is compared after unescaping.
        (r#"{"\u0024bytes": "AQID"}"#, Value::Bytes(vec![1, 2, 3])),
        (r#"{"$bytes": ""}"#, Value::Bytes(vec![])),
        (r#"{"$bytes": "\u0041QID"}"#, Value::Bytes(vec![1, 2, 3])),
        (r#"{"$f32vec": []}"#, Value::F32Vec(vec![])),
        (
            r#"{"$f32vec": [0.1, -2, 1e-45, 3.4028235e38, -0, {"$float": "NaN"}, { "$float" : "-Infinity" }]}"#,
            Value::F32Vec(vec![
                0.1,
                -2.0,
                1e-45,
                f32::MAX,
                -0.0,
                f32::NAN,
                f32::NEG_INFINITY,
            ]),
        ),
        (r#"{"$float": "NaN"}"#, Value::Float(f64::NAN)),
        (r#"{"$float": "Infinity"}"#, Value::Float(f64::INFINITY)),
        (
            r#" { "$float" : "-Infinity" } "#,
            Value::Float(f64::NEG_INFINITY),
        ),
        (
            r#"[{"$id":"00000000000000000000000000"},{"$ts":"1970-01-01T00:00:00Z"}]"#,
            Value::Array(vec![
                Value::Id(Id::from_u128(0)),
                Value::Timestamp(Timestamp::UNIX_EPOCH),
            ]),
        ),
    ];
    for (text, expected) in &cases {
        assert_identical(&json(text), expected, text);
    }
    // An f32 element is rounded once, from its text. This one is just
    // above the f32 halfway point 1 + 2^-24, so it rounds up; through f64
    // it would first round to that halfway point, then down to 1.0.
    let v = json(r#"{"$f32vec": [16777217, 1.0000000596046448]}"#);
    let up = f32::from_bits(0x3F80_0001);
    assert_identical(
        &v,
        &Value::F32Vec(vec![16_777_216.0, up]),
        "single rounding",
    );
    assert_eq!("1.0000000596046448".parse::<f64>().unwrap() as f32, 1.0);
}

#[test]
fn extended_json_rejects_malformed_forms() {
    let ext = |key, expected| K::ExtendedJson { key, expected };
    let alone = "to be the only key of its object";
    let f32_elements = "numbers, or `$float` objects for non-finite elements";
    let cases: Vec<(&str, JsonErrorKind, usize)> = vec![
        (r#"{"$id": 5}"#, ext("$id", "a string"), 9),
        (r#"{"$ts": null}"#, ext("$ts", "a string"), 9),
        (r#"{"$bytes": [1]}"#, ext("$bytes", "a string"), 12),
        (r#"{"$float": 1}"#, ext("$float", "a string"), 12),
        (
            r#"{"$float": "nan"}"#,
            ext("$float", "\"NaN\", \"Infinity\" or \"-Infinity\""),
            12,
        ),
        (
            r#"{"$f32vec": 1}"#,
            ext("$f32vec", "an array of numbers and `$float` objects"),
            13,
        ),
        (
            r#"{"$f32vec": ["a"]}"#,
            ext("$f32vec", "an array of numbers and `$float` objects"),
            14,
        ),
        (
            r#"{"$f32vec": [{"$fl": 1}]}"#,
            ext("$f32vec", f32_elements),
            15,
        ),
        (r#"{"$f32vec": [{}]}"#, ext("$f32vec", f32_elements), 15),
        (
            r#"{"$f32vec": [1e39]}"#,
            K::NumberOutOfRange { ty: "f32" },
            14,
        ),
        (
            r#"{"$f32vec": [1 2]}"#,
            K::Unexpected {
                found: '2',
                expected: "`,` or `]`",
            },
            16,
        ),
        (
            r#"{"$id": "01ARZ3NDEKTSV4RRFFQ69G5FAV", "b": 1}"#,
            ext("$id", alone),
            39,
        ),
        (
            r#"{"a": 1, "$ts": "1970-01-01T00:00:00Z"}"#,
            ext("$ts", alone),
            10,
        ),
        (
            r#"{"$id": "01ARZ3NDEKTSV4RRFFQ69G5FAV" 1}"#,
            K::Unexpected {
                found: '1',
                expected: "`}`",
            },
            38,
        ),
        (
            r#"{"$id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",}"#,
            K::TrailingComma,
            37,
        ),
        (
            r#"{"$id": "01ARZ3NDEKTSV4RRFFQ69G5FA"}"#,
            K::InvalidId(ParseIdError::Length { found: 25 }),
            9,
        ),
        (
            r#"{"$ts": "2262-04-11T23:47:16.854775808Z"}"#,
            K::InvalidTimestamp(CivilError::OutOfRange),
            9,
        ),
        (
            r#"{"$bytes": "AQI"}"#,
            K::InvalidBytes(Base64Error::Length { len: 3 }),
            12,
        ),
        // Text that ends inside a form is cut short, not malformed.
        (
            r#"{"$id":"#,
            K::UnexpectedEnd {
                expected: "a string",
            },
            8,
        ),
        (
            r#"{"$float": "#,
            K::UnexpectedEnd {
                expected: "a string",
            },
            12,
        ),
        (r#"{"$f32vec":"#, K::UnexpectedEnd { expected: "`[`" }, 12),
        (
            r#"{"$f32vec":[1,"#,
            K::UnexpectedEnd {
                expected: "a number or a `$float` object",
            },
            15,
        ),
        (
            r#"{"$f32vec":[{"#,
            K::UnexpectedEnd {
                expected: "`\"$float\"`",
            },
            14,
        ),
        (
            r#"{"$f32vec":[{"$float":"#,
            K::UnexpectedEnd {
                expected: "a string",
            },
            23,
        ),
    ];
    for (text, kind, column) in &cases {
        assert_error(Dialect::Json, text, kind, 1, *column);
    }
    let err = parse(r#"{"$id": "x"}"#, Limits::default()).unwrap_err();
    assert_eq!(
        Chain::new(&err).to_string(),
        "JSON at line 1, column 9: invalid `$id`: an id is 26 characters, not 1 bytes"
    );
    let err = parse(r#"{"$ts": "1970-01-01T24:00:00Z"}"#, Limits::default()).unwrap_err();
    assert_eq!(
        Chain::new(&err).to_string(),
        "JSON at line 1, column 9: invalid `$ts`: hour 24 is out of range"
    );
    let err = parse(r#"{"$bytes": "Zh=="}"#, Limits::default()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "JSON at line 1, column 12: invalid `$bytes`"
    );
    assert!(err.source().is_some());
    assert_eq!(
        Chain::new(&err).to_string(),
        "JSON at line 1, column 12: invalid `$bytes`: \
         base64 symbol at byte 1 has unused bits set, so the text is not canonical"
    );
}

#[test]
fn writers_have_one_text_per_value() {
    let id: Id = "01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap();
    let v = value!({
        "null": null,
        "bool": [true, false],
        "int": -7,
        "float": [1.0, -0.0, 0.25],
        "text": "tab\there \"quoted\" \u{1}",
        "bytes": (Value::Bytes(vec![1, 2, 3])),
        "ts": (Timestamp::from_nanos(1)),
        "id": id,
        "empty": [[], {}],
        "vec": (vec![0.5f32, f32::NAN]),
        "nan": (f64::NAN),
    });
    let compact = r#"{"null":null,"bool":[true,false],"int":-7,"float":[1.0,-0.0,0.25],"text":"tab\there \"quoted\" \u0001","bytes":{"$bytes":"AQID"},"ts":{"$ts":"1970-01-01T00:00:00.000000001Z"},"id":{"$id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"},"empty":[[],{}],"vec":{"$f32vec":[0.5,{"$float":"NaN"}]},"nan":{"$float":"NaN"}}"#;
    assert_eq!(to_string_compact(&v), compact);
    assert_eq!(v.to_string(), compact);
    let pretty = r#"{
  "null": null,
  "bool": [
    true,
    false
  ],
  "int": -7,
  "float": [
    1.0,
    -0.0,
    0.25
  ],
  "text": "tab\there \"quoted\" \u0001",
  "bytes": {"$bytes": "AQID"},
  "ts": {"$ts": "1970-01-01T00:00:00.000000001Z"},
  "id": {"$id": "01ARZ3NDEKTSV4RRFFQ69G5FAV"},
  "empty": [
    [],
    {}
  ],
  "vec": {"$f32vec": [0.5, {"$float": "NaN"}]},
  "nan": {"$float": "NaN"}
}"#;
    assert_eq!(to_string_pretty(&v), pretty);
    assert_identical(&json(compact), &v, "compact");
    assert_identical(&json(pretty), &v, "pretty");
    assert_eq!(to_string_pretty(&Value::Int(1)), "1");
    assert_eq!(to_string_pretty(&value!([])), "[]");
}

#[test]
fn floats_are_written_shortest_with_a_point_or_an_exponent() {
    let cases: [(f64, &str); 22] = [
        (0.0, "0.0"),
        (-0.0, "-0.0"),
        (5.0, "5.0"),
        (-5.0, "-5.0"),
        (0.1, "0.1"),
        (123_456.789, "123456.789"),
        (1e15, "1000000000000000.0"),
        (9_999_999_999_999_998.0, "9999999999999998.0"),
        (1e16, "1e16"),
        (0.0001, "0.0001"),
        (0.000_15, "0.00015"),
        (0.000_01, "1e-5"),
        (-1.5e-7, "-1.5e-7"),
        (1.5e300, "1.5e300"),
        (5e-324, "5e-324"),
        (f64::MAX, "1.7976931348623157e308"),
        (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
        (9_007_199_254_740_993.0, "9007199254740992.0"),
        (9_223_372_036_854_775_808.0, "9.223372036854776e18"),
        (f64::INFINITY, r#"{"$float":"Infinity"}"#),
        (f64::NEG_INFINITY, r#"{"$float":"-Infinity"}"#),
        (-f64::NAN, r#"{"$float":"NaN"}"#),
    ];
    for (x, text) in cases {
        assert_eq!(to_string_compact(&Value::Float(x)), text, "{x:e}");
    }
    // An exact tie between two shortest candidates rounds up.
    // 446.578125 exactly: 446.57812 and 446.57813 are equally near.
    let tie = f32::from_bits(0x43DF_4A00);
    let v = Value::F32Vec(vec![tie, 1e-45, f32::MAX, -0.0]);
    assert_eq!(
        to_string_compact(&v),
        r#"{"$f32vec":[446.57813,1e-45,3.4028235e38,-0.0]}"#
    );
}

/// The writer's texts for many inputs, one per line, and their `fnv1a64`.
fn aggregate(lines: impl IntoIterator<Item = String>) -> (usize, u64) {
    let mut all = String::new();
    let mut count = 0;
    for line in lines {
        all.push_str(&line);
        all.push('\n');
        count += 1;
    }
    (count, fnv1a64(all.as_bytes()))
}

// The pinned values below come from an independent Python program that
// mirrors each generator call for call: shortest digits found by exact
// fraction search (ties rounded up, otherwise checked against `repr`),
// `json.dumps(ensure_ascii=False)`, and `float()`/`int()`.

#[test]
fn f64_text_matches_the_reference() {
    let mut rng = Pcg32::new(0x5EED, 1);
    let mut texts = Vec::new();
    for i in 0..20_000 {
        let bits = if i % 2 == 0 {
            rng.next_u64()
        } else {
            let sign = u64::from(rng.next_u32() & 1);
            let exponent = 993 + u64::from(rng.next_u32() % 80);
            let mantissa = rng.next_u64() & ((1 << 52) - 1);
            sign << 63 | exponent << 52 | mantissa
        };
        let x = f64::from_bits(bits);
        if x.is_finite() {
            let text = to_string_compact(&Value::Float(x));
            assert_identical(&json(&text), &Value::Float(x), &text);
            texts.push(text);
        }
    }
    assert_eq!(aggregate(texts), (19_996, 0x248C_7749_95DC_6D21));
}

#[test]
fn f32_text_matches_the_reference() {
    let mut rng = Pcg32::new(0x5EED, 2);
    let mut texts = Vec::new();
    for i in 0..20_000 {
        let bits = if i % 2 == 0 {
            rng.next_u32()
        } else {
            let sign = rng.next_u32() & 1;
            let exponent = 107 + rng.next_u32() % 60;
            let mantissa = rng.next_u32() & 0x7F_FFFF;
            sign << 31 | exponent << 23 | mantissa
        };
        let x = f32::from_bits(bits);
        if x.is_finite() {
            let text = to_string_compact(&Value::F32Vec(vec![x]));
            assert_identical(&json(&text), &Value::F32Vec(vec![x]), &text);
            let element = text
                .strip_prefix(r#"{"$f32vec":["#)
                .and_then(|t| t.strip_suffix("]}"))
                .unwrap();
            texts.push(element.to_owned());
        }
    }
    assert_eq!(aggregate(texts), (19_965, 0xFA24_921D_E28C_F9CE));
}

#[test]
fn string_escapes_match_the_reference() {
    let mut alphabet: Vec<char> = ('\0'..='\u{20}').collect();
    alphabet.extend([
        '"',
        '\\',
        '/',
        'a',
        'Z',
        '~',
        '\u{7f}',
        '\u{80}',
        'é',
        '\u{2028}',
        '\u{2029}',
        '\u{d7ff}',
        '\u{e000}',
        '\u{feff}',
        '\u{fffd}',
        '\u{ffff}',
        '😀',
        '\u{10ffff}',
    ]);
    let mut rng = Pcg32::new(0x5EED, 3);
    let mut texts = Vec::new();
    for _ in 0..2000 {
        let len = rng.next_u32() % 24;
        let s: String = (0..len)
            .map(|_| alphabet[(rng.next_u32() as usize) % alphabet.len()])
            .collect();
        let text = to_string_compact(&Value::String(s.clone()));
        assert_identical(&json(&text), &Value::String(s), &text);
        texts.push(text);
    }
    assert_eq!(aggregate(texts), (2000, 0x2E1A_94CF_43DC_FE89));
}

#[test]
fn number_parsing_matches_the_reference() {
    fn digits(rng: &mut Pcg32, n: u32, first_nonzero: bool, out: &mut String) {
        for i in 0..n {
            let d = if i == 0 && first_nonzero {
                1 + rng.next_u32() % 9
            } else {
                rng.next_u32() % 10
            };
            out.push(char::from_digit(d, 10).unwrap());
        }
    }
    let mut rng = Pcg32::new(0x5EED, 4);
    let mut lines = Vec::new();
    for _ in 0..20_000 {
        let mut s = String::new();
        if rng.next_u32().is_multiple_of(3) {
            s.push('-');
        }
        let n = 1 + rng.next_u32() % 25;
        digits(&mut rng, n, n > 1, &mut s);
        if rng.next_u32().is_multiple_of(2) {
            s.push('.');
            let n = 1 + rng.next_u32() % 25;
            digits(&mut rng, n, false, &mut s);
        }
        if rng.next_u32().is_multiple_of(3) {
            s.push(['e', 'E'][(rng.next_u32() % 2) as usize]);
            s.push_str(["", "+", "-"][(rng.next_u32() % 3) as usize]);
            let n = 1 + rng.next_u32() % 3;
            digits(&mut rng, n, false, &mut s);
        }
        let line = match parse(&s, Limits::default()) {
            Ok(Value::Int(i)) => format!("{s} I {i}"),
            Ok(Value::Float(f)) => format!("{s} F {:016x}", f.to_bits()),
            Err(e) if e.kind() == &K::NumberOutOfRange { ty: "f64" } => format!("{s} R"),
            other => panic!("{s}: {other:?}"),
        };
        lines.push(line);
    }
    assert_eq!(aggregate(lines), (20_000, 0x9052_D16C_29F5_7686));
}

/// The text of an `f32` element as the writer gives it.
fn f32_text(x: f32) -> String {
    let text = to_string_compact(&Value::F32Vec(vec![x]));
    text.strip_prefix(r#"{"$f32vec":["#)
        .and_then(|t| t.strip_suffix("]}"))
        .unwrap()
        .to_owned()
}

#[test]
fn short_floats_match_the_reference() {
    // Few digits and a small exponent: where the plain layout pads.
    let mut texts = Vec::new();
    for e in -7..18 {
        for k in 1..100 {
            for sign in ["", "-"] {
                let x: f64 = format!("{sign}{k}e{e}").parse().unwrap();
                texts.push(to_string_compact(&Value::Float(x)));
            }
        }
    }
    for k in -2000..=2000 {
        texts.push(to_string_compact(&Value::Float(f64::from(k) / 8.0)));
    }
    assert_eq!(aggregate(texts), (8951, 0x40BE_9820_0C41_E85A));
    let mut texts = Vec::new();
    for e in -7..13 {
        for k in 1..100 {
            for sign in ["", "-"] {
                texts.push(f32_text(format!("{sign}{k}e{e}").parse().unwrap()));
            }
        }
    }
    for k in -2000i16..=2000 {
        texts.push(f32_text(f32::from(k) / 8.0));
    }
    assert_eq!(aggregate(texts), (7961, 0x69CA_4506_8FD5_1869));
}

#[test]
fn long_numbers_match_the_reference() {
    fn digits(rng: &mut Pcg32, n: u32, first_nonzero: bool, out: &mut String) {
        for i in 0..n {
            let d = if i == 0 && first_nonzero {
                1 + rng.next_u32() % 9
            } else {
                rng.next_u32() % 10
            };
            out.push(char::from_digit(d, 10).unwrap());
        }
    }
    let mut rng = Pcg32::new(0x5EED, 7);
    let mut lines = Vec::new();
    for _ in 0..5000 {
        let mut s = String::new();
        if rng.next_u32().is_multiple_of(2) {
            s.push('-');
        }
        let n = 1 + rng.next_u32() % 120;
        digits(&mut rng, n, n > 1, &mut s);
        if rng.next_u32().is_multiple_of(2) {
            s.push('.');
            let n = 1 + rng.next_u32() % 120;
            digits(&mut rng, n, false, &mut s);
        }
        if rng.next_u32().is_multiple_of(2) {
            s.push('e');
            s.push_str(["", "+", "-"][(rng.next_u32() % 3) as usize]);
            let n = 1 + rng.next_u32() % 4;
            digits(&mut rng, n, false, &mut s);
        }
        lines.push(match parse(&s, Limits::default()) {
            Ok(Value::Int(i)) => format!("I {i}"),
            Ok(Value::Float(f)) => format!("F {:016x}", f.to_bits()),
            Err(e) if e.kind() == &K::NumberOutOfRange { ty: "f64" } => "R".to_owned(),
            other => panic!("{s}: {other:?}"),
        });
    }
    assert_eq!(aggregate(lines), (5000, 0x25F3_041A_C897_2969));

    let mut rng = Pcg32::new(0x5EED, 8);
    let mut lines = Vec::new();
    for _ in 0..2000 {
        let mut s = String::new();
        if rng.next_u32().is_multiple_of(2) {
            s.push('-');
        }
        let n = 1 + rng.next_u32() % 120;
        digits(&mut rng, n, n > 1, &mut s);
        if rng.next_u32().is_multiple_of(2) {
            s.push('.');
            let n = 1 + rng.next_u32() % 120;
            digits(&mut rng, n, false, &mut s);
        }
        s.push_str(&format!("e-{}", rng.next_u32() % 160));
        lines.push(
            match parse(&format!(r#"{{"$f32vec":[{s}]}}"#), Limits::default()) {
                Ok(Value::F32Vec(v)) => format!("{:08x}", v[0].to_bits()),
                Err(e) if e.kind() == &K::NumberOutOfRange { ty: "f32" } => "R".to_owned(),
                other => panic!("{s}: {other:?}"),
            },
        );
    }
    assert_eq!(aggregate(lines), (2000, 0xE5EC_25B4_F67D_F3C0));
}

#[test]
fn huge_exponents_offset_by_many_digits_are_exact() {
    // Each is exactly 1. `from_str` alone stops adding exponent digits once
    // the exponent reaches 65,536, so it gets 655,360 and up wrong; the
    // smaller ones are controls.
    for n in [65_530, 65_536, 70_000, 655_360, 1_000_000] {
        let big = format!("1{}e-{n}", "0".repeat(n));
        let small = format!("0.{}1e{n}", "0".repeat(n - 1));
        for text in [&big, &small] {
            assert_identical(&json(text), &Value::Float(1.0), &text[..12]);
            let vector = json(&format!(r#"{{"$f32vec":[{text}]}}"#));
            assert_identical(&vector, &Value::F32Vec(vec![1.0]), &text[..12]);
        }
    }
    let ones = format!("{}e-1999999", "1".repeat(2_000_000));
    assert_identical(&json(&ones), &Value::Float(1.111_111_111_111_111_2), "ones");
    let zeros = |sign: &str| format!("{sign}0.{}e99999999999999999999", "0".repeat(100));
    assert_identical(&json(&zeros("")), &Value::Float(0.0), "zero");
    assert_identical(&json(&zeros("-")), &Value::Float(-0.0), "negative zero");
    let tiny = format!("-{}e-99999999999999999999", "9".repeat(100));
    assert_identical(&json(&tiny), &Value::Float(-0.0), "underflow");
    let err = parse(
        &format!("{}e99999999999999999999", "9".repeat(100)),
        Limits::default(),
    );
    assert_eq!(err.unwrap_err().kind(), &K::NumberOutOfRange { ty: "f64" });
    // 64 bytes and fewer go to `from_str` as they are; longer ones are
    // rewritten first. Both read the same.
    for zeros in [59, 60] {
        let edge = format!("1.{}e-1", "0".repeat(zeros));
        assert_eq!(edge.len(), 5 + zeros);
        assert_identical(&json(&edge), &Value::Float(0.1), &edge);
    }
}

/// The decimal digits of `base^exponent × factor`, most significant first:
/// exact halfway points, built without the parser.
fn decimal(base: u64, exponent: u32, factor: u64) -> String {
    let mut digits = vec![1u8]; // least significant first
    let mut multiply = |m: u64| {
        let mut carry = 0u128;
        for d in &mut digits {
            let x = u128::from(*d) * u128::from(m) + carry;
            *d = (x % 10) as u8;
            carry = x / 10;
        }
        while carry > 0 {
            digits.push((carry % 10) as u8);
            carry /= 10;
        }
    };
    for _ in 0..exponent {
        multiply(base);
    }
    multiply(factor);
    digits.iter().rev().map(|d| char::from(b'0' + d)).collect()
}

#[test]
fn long_numbers_round_halfway_points_correctly() {
    let f64_of = |text: &str| match json(text) {
        Value::Float(f) => f.to_bits(),
        other => panic!("{other:?}"),
    };
    // 2^-1075 = 5^1075 × 10^-1075 (752 digits): halfway from 0 to the
    // smallest subnormal.
    let h = decimal(5, 1075, 1);
    assert_eq!(h.len(), 752);
    assert_eq!(f64_of(&format!("{h}e-1075")), 0, "a tie goes to even, 0");
    let above = format!("{h}{}1e-{}", "0".repeat(60), 1075 + 61);
    assert_eq!(f64_of(&above), 1, "a 1 past digit 800 is kept");
    let mut below = h.clone();
    below.pop();
    let below = format!("{below}4{}e-{}", "9".repeat(60), 1075 + 60);
    assert_eq!(f64_of(&below), 0);
    let above_at_753 = format!("{h}1e-1076");
    assert_eq!(f64_of(&above_at_753), 1);
    // (2^53 - 1) × 2^-1075 (768 digits): halfway from the largest
    // subnormal to `f64::MIN_POSITIVE`, whose mantissa is even.
    let h2 = decimal(5, 1075, (1 << 53) - 1);
    assert_eq!(h2.len(), 768);
    assert_eq!(f64_of(&format!("{h2}e-1075")), f64::MIN_POSITIVE.to_bits());
    let mut below = h2.clone();
    below.pop();
    let below = format!("{below}4e-1075");
    assert_eq!(f64_of(&below), 0x000F_FFFF_FFFF_FFFF);
    // 1 + 2^-53 = (2^53 + 1) × 5^53 × 10^-53, padded past 64 bytes.
    let one = decimal(5, 53, (1 << 53) + 1);
    let tie = format!("{one}{}e-153", "0".repeat(100));
    assert_eq!(f64_of(&tie), 1f64.to_bits());
    let up = format!("{one}{}1e-{}", "0".repeat(900), 53 + 901);
    assert_eq!(f64_of(&up), 0x3FF0_0000_0000_0001);
    // f32: 2^-150 = 5^150 × 10^-150, halfway from 0 to `1e-45`.
    let h = decimal(5, 150, 1);
    let f32_of = |text: &str| match json(&format!(r#"{{"$f32vec":[{text}]}}"#)) {
        Value::F32Vec(v) => v[0].to_bits(),
        other => panic!("{other:?}"),
    };
    assert_eq!(f32_of(&format!("{h}e-150")), 0);
    assert_eq!(
        f32_of(&format!("{h}{}1e-{}", "0".repeat(900), 150 + 901)),
        1
    );
}

#[test]
fn long_numbers_at_the_ends_of_the_range() {
    let pad = "0".repeat(70);
    let cases: [(String, Option<u64>); 8] = [
        (format!("4.9{pad}e-324"), Some(1)),
        (format!("2.5{pad}e-324"), Some(1)),
        (format!("2.4{pad}e-324"), Some(0)),
        (format!("1.{pad}e-323"), Some(2)),
        (format!("-1.{pad}e-400"), Some((-0f64).to_bits())),
        (format!("1.{pad}e308"), Some(1e308f64.to_bits())),
        (
            format!("1.7976931348623157{pad}e308"),
            Some(f64::MAX.to_bits()),
        ),
        (format!("1.8{pad}e308"), None),
    ];
    for (text, expected) in &cases {
        let found = match parse(text, Limits::default()) {
            Ok(Value::Float(f)) => Some(f.to_bits()),
            Err(e) if e.kind() == &K::NumberOutOfRange { ty: "f64" } => None,
            other => panic!("{text}: {other:?}"),
        };
        assert_eq!(found, *expected, "{}…", &text[..6]);
    }
}

#[test]
fn wrappers_cannot_nest_to_overflow_the_stack() {
    // Each wrapper's value has its own reader, so these fail at once
    // instead of recursing a million times. Run on a 2 MiB stack.
    let n = 1_000_000;
    let inputs = [
        format!("{}\"NaN\"{}", r#"{"$float":"#.repeat(n), "}".repeat(n)),
        format!("{}\"x\"{}", r#"{"$id":"#.repeat(n), "}".repeat(n)),
        format!(
            "{}\"NaN\"{}",
            r#"{"$f32vec":[{"$float":"#.repeat(n),
            "}]}".repeat(n)
        ),
    ];
    let mut limits = Limits::default();
    limits.max_document_bytes = 64 << 20;
    let errors = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || inputs.map(|text| parse(&text, limits).map(|_| ())))
        .unwrap()
        .join()
        .unwrap();
    let ext = |key, expected| K::ExtendedJson { key, expected };
    assert_eq!(
        errors[0].as_ref().unwrap_err().kind(),
        &ext("$float", "a string")
    );
    assert_eq!(
        errors[1].as_ref().unwrap_err().kind(),
        &ext("$id", "a string")
    );
    assert_eq!(
        errors[2].as_ref().unwrap_err().kind(),
        &ext("$float", "a string")
    );
}

/// Nesting that `parse` and every recursive operation on the result
/// handle on a 2 MiB stack, in a debug build.
const SUPPORTED_DEPTH: usize = 256;

#[test]
fn the_supported_depth_works_on_a_small_stack() {
    let mut limits = Limits::default();
    limits.max_depth = SUPPORTED_DEPTH;
    let mut text = String::new();
    for i in 0..SUPPORTED_DEPTH {
        text.push_str(if i % 2 == 0 { "[" } else { r#"{"k":"# });
    }
    text.push_str(r#"[{"$id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"},{"$f32vec":[{"$float":"NaN"}]},-0.0]"#);
    for i in (0..SUPPORTED_DEPTH).rev() {
        text.push(if i % 2 == 0 { ']' } else { '}' });
    }
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let err = parse(&text, Limits::default()).unwrap_err();
            assert_eq!(err.kind(), &K::TooDeep { max: 64 });
            // The innermost array is one level past the others.
            limits.max_depth = SUPPORTED_DEPTH + 1;
            let v = parse(&text, limits).unwrap();
            let copy = v.clone();
            assert!(v.identical(&copy) && v == copy && v.cmp(&copy).is_eq());
            let mut h = softshell_core::hash::FastHasher::default();
            std::hash::Hash::hash(&v, &mut h);
            let _ = format!("{v:?}");
            assert_eq!(to_string_compact(&v), text);
            let pretty = to_string_pretty(&v);
            assert!(parse(&pretty, limits).unwrap().identical(&v));
            drop((v, copy));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn extended_json_round_trips_every_variant() {
    check_sized("extended JSON round-trips", 3000, test_values::wide, |v| {
        let compact = to_string_compact(v);
        for text in [compact.clone(), to_string_pretty(v)] {
            for (dialect, parsed) in [
                ("JSON", parse(&text, Limits::default())),
                ("JSONC", jsonc::parse(&text, Limits::default())),
            ] {
                let parsed = parsed.map_err(|e| format!("{dialect} {e} in {text}"))?;
                if !parsed.identical(v) {
                    return Err(format!("{dialect} read {text} as {parsed:?}"));
                }
                if parsed != *v {
                    return Err(format!("{parsed:?} is not equal to itself"));
                }
            }
        }
        if to_string_compact(&json(&compact)) != compact {
            return Err(format!("{compact} is not written the same way twice"));
        }
        Ok(())
    });
}

/// A JSON text with 1 to 8 random edits: bytes flipped to JSON's
/// punctuation or anything else, inserted, deleted, duplicated or cut.
fn mutated(rng: &mut Pcg32) -> (String, bool) {
    const PUNCTUATION: &[u8] = b"\"\\{}[],:/*-+.0123456789eEu \n\rtfnaxD$";
    let base = match rng.range(0..4) {
        0 => to_string_pretty(&test_values::wide(rng, 40)),
        1 => {
            let v = test_values::wide(rng, 30);
            format!("/* c */ {} // c", to_string_compact(&v))
        }
        _ => to_string_compact(&test_values::wide(rng, 40)),
    };
    let mut bytes = base.into_bytes();
    for _ in 0..rng.range(1..9) {
        let len = bytes.len() as u64;
        let at = rng.range(0..len + 1) as usize;
        let byte = if rng.next_u32().is_multiple_of(2) {
            PUNCTUATION[rng.range(0..PUNCTUATION.len() as u64) as usize]
        } else {
            rng.next_u32() as u8
        };
        match rng.range(0..5) {
            0 if at < bytes.len() => bytes[at] = byte,
            1 => bytes.insert(at, byte),
            2 if at < bytes.len() => {
                bytes.remove(at);
            }
            3 if at < bytes.len() => {
                let end = (at + rng.range(1..16) as usize).min(bytes.len());
                let copy = bytes[at..end].to_vec();
                bytes.splice(at..at, copy);
            }
            _ => bytes.truncate(at),
        }
    }
    let jsonc = rng.next_u32().is_multiple_of(2);
    (String::from_utf8_lossy(&bytes).into_owned(), jsonc)
}

#[test]
fn malformed_input_returns_errors() {
    let mut tight = Limits::default();
    tight.max_depth = 3;
    tight.max_entries = 4;
    tight.max_string_bytes = 8;
    tight.max_vector_dims = 2;
    check(
        "malformed JSON never panics",
        20_000,
        mutated,
        |(text, jsonc)| {
            for limits in [Limits::default(), tight] {
                let result = if *jsonc {
                    jsonc::parse(text, limits)
                } else {
                    parse(text, limits)
                };
                match result {
                    // What parses must write and read back the same.
                    Ok(v) => {
                        let again = parse(&to_string_compact(&v), Limits::default())
                            .map_err(|e| format!("rewritten text fails: {e}"))?;
                        if !again.identical(&v) {
                            return Err(format!("rewritten as {again:?}"));
                        }
                    }
                    Err(e) => {
                        if e.offset() > text.len() || e.line() == 0 || e.column() == 0 {
                            return Err(format!("bad position in {e:?}"));
                        }
                        let _ = e.to_string();
                    }
                }
            }
            Ok(())
        },
    );
}
