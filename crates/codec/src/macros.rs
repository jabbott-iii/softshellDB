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

//! The [`value!`](crate::value::value) macro, defined here and exported
//! from [`value`](crate::value).

/// Builds a [`Value`](crate::value::Value) with JSON-like syntax.
///
/// - `null`, `[...]` and `{...}` are JSON's; anything else is a Rust
///   expression, converted with `Value::from` and moved, so `true`, `1`,
///   `-2.5`, `"text"`, a variable or a call all work.
/// - An object key is one token: a string literal, a variable, or a
///   parenthesized expression, converted with `String::from`.
/// - Trailing commas are allowed.
/// - A repeated key replaces the earlier value in its place, as
///   [`Object::insert`](crate::value::Object::insert) does.
/// - Extended JSON is not interpreted: `value!({"$id": "01AR…"})` is an
///   object holding a string. Write `Value::Id(id)` for an id.
///
/// ```
/// use softshell_codec::value::{value, Value};
///
/// let name = "tide";
/// let v = value!({
///     "name": name,
///     "depth": -2.5,
///     "tags": ["ocean", null, [1, 2]],
///     (format!("k{}", 1)): { "nested": true },
/// });
/// let object = v.as_object().unwrap();
/// assert_eq!(object.get("name"), Some(&Value::from("tide")));
/// assert_eq!(object.get("depth"), Some(&Value::Float(-2.5)));
/// assert_eq!(object.keys().collect::<Vec<_>>(), ["name", "depth", "tags", "k1"]);
/// ```
///
/// Limits:
///
/// - An integer literal is an `i32` unless it has a suffix, so write
///   `1_730_000_000_000i64` for a larger one. `usize` and `u64` do not
///   convert (`n as i64`, or `i64::try_from(n)?`), nor do vectors of
///   integers (`Value::Bytes(v)` for bytes). A vector of `f32`s is an
///   `F32Vec`.
/// - A block value needs parentheses: `({ let x = 1; x })`.
/// - A list (array or object) made only of single token trees expands
///   without recursion, however long. If any element or entry has more
///   than one token (`-1`, `a + b`, `f(x)`), the whole list is split one
///   element per level of macro recursion, single tokens included, and a
///   list split this way inside another adds to the levels its parent
///   already uses. The default `recursion_limit` of 128 leaves about 125
///   such levels along one path of nesting; raise it with
///   `#![recursion_limit]` or build large values with
///   [`Object::insert`](crate::value::Object::insert).
#[doc(hidden)]
#[macro_export]
macro_rules! __value {
    // Array elements, split at top-level commas.
    (@array [$($done:expr),*]) => {
        ::std::vec::Vec::from([$($done),*])
    };
    (@array [$($done:expr),*] $element:tt $(, $($rest:tt)*)?) => {
        $crate::__value!(@array [$($done,)* $crate::__value!($element)] $($($rest)*)?)
    };
    (@array [$($done:expr),*] $element:expr $(, $($rest:tt)*)?) => {
        $crate::__value!(@array [$($done,)* $crate::__value!(@from $element)] $($($rest)*)?)
    };
    // Object entries, split at top-level commas and inserted.
    (@object $object:ident) => {};
    (@object $object:ident $key:tt : $value:tt $(, $($rest:tt)*)?) => {
        $object.insert($crate::__value!(@key $key), $crate::__value!($value));
        $crate::__value!(@object $object $($($rest)*)?);
    };
    (@object $object:ident $key:tt : $value:expr $(, $($rest:tt)*)?) => {
        $object.insert($crate::__value!(@key $key), $crate::__value!(@from $value));
        $crate::__value!(@object $object $($($rest)*)?);
    };
    // A key, without the parentheses around an expression, which would
    // otherwise warn at the caller.
    (@key ($key:expr)) => {
        $key
    };
    (@key $key:tt) => {
        $key
    };
    // The one place a Rust value converts.
    (@from $value:expr) => {
        $crate::value::Value::from($value)
    };
    (null) => {
        $crate::value::Value::Null
    };
    ([]) => {
        $crate::value::Value::Array(::std::vec::Vec::new())
    };
    // Every element one token tree: no recursion.
    ([ $($element:tt),+ $(,)? ]) => {
        $crate::value::Value::Array(::std::vec::Vec::from([$($crate::__value!($element)),+]))
    };
    ([ $($elements:tt)+ ]) => {
        $crate::value::Value::Array($crate::__value!(@array [] $($elements)+))
    };
    ({}) => {
        $crate::value::Value::Object($crate::value::Object::new())
    };
    // Every value one token tree: no recursion.
    ({ $($key:tt : $value:tt),+ $(,)? }) => {{
        let mut object = $crate::value::Object::new();
        $(object.insert($crate::__value!(@key $key), $crate::__value!($value));)+
        $crate::value::Value::Object(object)
    }};
    ({ $($entries:tt)+ }) => {{
        let mut object = $crate::value::Object::new();
        $crate::__value!(@object object $($entries)+);
        $crate::value::Value::Object(object)
    }};
    ($other:expr) => {
        $crate::__value!(@from $other)
    };
}

#[cfg(test)]
mod tests {
    use crate::value::{Object, Value, value};

    fn object(entries: Vec<(&str, Value)>) -> Value {
        let entries: Vec<(String, Value)> = entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect();
        Value::Object(Object::try_from(entries).unwrap())
    }

    #[test]
    fn scalars() {
        assert!(value!(null).identical(&Value::Null));
        assert!(value!(true).identical(&Value::Bool(true)));
        assert!(value!(false).identical(&Value::Bool(false)));
        assert!(value!(1).identical(&Value::Int(1)));
        assert!(value!(-1).identical(&Value::Int(-1)));
        assert!(value!(i64::MIN).identical(&Value::Int(i64::MIN)));
        assert!(value!(2.5).identical(&Value::Float(2.5)));
        assert!(value!(-0.0).identical(&Value::Float(-0.0)));
        assert!(value!("s").identical(&Value::String("s".into())));
        assert!(value!(String::from("t")).identical(&Value::String("t".into())));
        assert!(value!(&String::from("r")).identical(&Value::String("r".into())));
        assert!(value!(Value::Bytes(vec![1, 2])).identical(&Value::Bytes(vec![1, 2])));
        assert!(value!(vec![1.0f32]).identical(&Value::F32Vec(vec![1.0])));
        assert!(value!(None::<i32>).identical(&Value::Null));
        assert!(value!(Some(3)).identical(&Value::Int(3)));
    }

    #[test]
    fn arrays() {
        assert!(value!([]).identical(&Value::Array(vec![])));
        let expected = Value::Array(vec![
            Value::Int(1),
            Value::Null,
            Value::Array(vec![Value::from("a"), Value::Array(vec![])]),
            object(vec![("k", Value::Bool(false))]),
        ]);
        assert!(value!([1, null, ["a", []], {"k": false}]).identical(&expected));
        assert!(value!([1, null, ["a", []], {"k": false},]).identical(&expected));
        // Multi-token elements take the munching path.
        let x: i32 = 4;
        let munched = value!([-1, x + 1, null, [2], { "a": -x }, i64::from(x),]);
        let expected = Value::Array(vec![
            Value::Int(-1),
            Value::Int(5),
            Value::Null,
            Value::Array(vec![Value::Int(2)]),
            object(vec![("a", Value::Int(-4))]),
            Value::Int(4),
        ]);
        assert!(munched.identical(&expected), "{munched:?}");
    }

    #[test]
    fn objects() {
        assert!(value!({}).identical(&Value::Object(Object::new())));
        let key = "k";
        let owned = String::from("o");
        let v = value!({ "a": 1, key: [true], (format!("{key}2")): {}, owned: null });
        let expected = object(vec![
            ("a", Value::Int(1)),
            ("k", Value::Array(vec![Value::Bool(true)])),
            ("k2", Value::Object(Object::new())),
            ("o", Value::Null),
        ]);
        assert!(v.identical(&expected), "{v:?}");
        let n: i64 = 2;
        let munched = value!({ "a": -1, "b": n * 3, "c": [null], "d": n.pow(2), });
        let expected = object(vec![
            ("a", Value::Int(-1)),
            ("b", Value::Int(6)),
            ("c", Value::Array(vec![Value::Null])),
            ("d", Value::Int(4)),
        ]);
        assert!(munched.identical(&expected), "{munched:?}");
    }

    #[test]
    fn a_repeated_key_replaces_the_value_in_place() {
        let v = value!({ "a": 1, "b": 2, "a": 3 });
        assert!(v.identical(&object(vec![("a", Value::Int(3)), ("b", Value::Int(2))])));
        let v = value!({ "a": -1, "b": 2, "a": -3 });
        assert!(v.identical(&object(vec![("a", Value::Int(-3)), ("b", Value::Int(2))])));
    }

    #[test]
    fn the_architecture_example_builds() {
        fn embedding_from_somewhere() -> Vec<f32> {
            vec![0.25, 0.5]
        }
        let v = value!({
            "title": "Tide tables",
            "tags": ["ocean", "notes"],
            "body": "Low tide is the best time to explore tide pools.",
            "embedding": embedding_from_somewhere(),
        });
        let object = v.as_object().unwrap();
        assert_eq!(object.len(), 4);
        assert!(
            object
                .get("embedding")
                .unwrap()
                .identical(&Value::F32Vec(vec![0.25, 0.5]))
        );
        assert!(value!({ "count": 1 }).identical(&self::object(vec![("count", Value::Int(1))])));
    }

    #[test]
    fn long_literals_stay_within_the_default_recursion_limit() {
        // One token tree per element: no recursion, however long.
        let v = value!([
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8,
            9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7,
            8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6,
            7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5,
            6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4,
            5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9
        ]);
        assert_eq!(v.as_array().unwrap().len(), 170);
        let nested = value!([[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[1]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]);
        let mut depth = 0;
        let mut at = &nested;
        while let Some([inner]) = at.as_array() {
            depth += 1;
            at = inner;
        }
        assert_eq!(depth, 32);
    }

    #[test]
    fn reserved_keys_are_plain_keys() {
        let v = value!({ "$id": "01ARZ3NDEKTSV4RRFFQ69G5FAV", "$in": [1] });
        let object = v.as_object().unwrap();
        assert!(
            object
                .get("$id")
                .unwrap()
                .identical(&Value::from("01ARZ3NDEKTSV4RRFFQ69G5FAV"))
        );
    }

    struct Note {
        title: String,
        body: String,
        tags: Vec<String>,
        stars: i32,
    }

    #[test]
    fn a_wide_object_of_method_calls_compiles() {
        let n = Note {
            title: "Tide tables".into(),
            body: "Low tide".into(),
            tags: vec!["ocean".into()],
            stars: 4,
        };
        // 48 entries, every value an expression of several tokens.
        let v = value!({
            "t0": n.title.clone(), "b0": &n.body, "g0": n.tags.len() as i64, "s0": -n.stars,
            "t1": n.title.clone(), "b1": &n.body, "g1": n.tags.len() as i64, "s1": -n.stars,
            "t2": n.title.clone(), "b2": &n.body, "g2": n.tags.len() as i64, "s2": -n.stars,
            "t3": n.title.clone(), "b3": &n.body, "g3": n.tags.len() as i64, "s3": -n.stars,
            "t4": n.title.clone(), "b4": &n.body, "g4": n.tags.len() as i64, "s4": -n.stars,
            "t5": n.title.clone(), "b5": &n.body, "g5": n.tags.len() as i64, "s5": -n.stars,
            "t6": n.title.clone(), "b6": &n.body, "g6": n.tags.len() as i64, "s6": -n.stars,
            "t7": n.title.clone(), "b7": &n.body, "g7": n.tags.len() as i64, "s7": -n.stars,
            "t8": n.title.clone(), "b8": &n.body, "g8": n.tags.len() as i64, "s8": -n.stars,
            "t9": n.title.clone(), "b9": &n.body, "g9": n.tags.len() as i64, "s9": -n.stars,
            "ta": n.title.clone(), "ba": &n.body, "ga": n.tags.len() as i64, "sa": -n.stars,
            "tb": n.title.clone(), "bb": &n.body, "gb": n.tags.len() as i64, "sb": -n.stars,
        });
        let object = v.as_object().unwrap();
        assert_eq!(object.len(), 48);
        assert!(object.get("sb").unwrap().identical(&Value::Int(-4)));
        assert!(
            object
                .get("b5")
                .unwrap()
                .identical(&Value::from("Low tide"))
        );
        let generic = value!([
            std::collections::BTreeMap::<String, i32>::new().len() as i64,
            5
        ]);
        assert!(generic.identical(&Value::Array(vec![Value::Int(0), Value::Int(5)])));
    }
}
