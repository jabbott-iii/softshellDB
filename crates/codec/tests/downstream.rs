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

//! The value model, `value!` and JSON as other crates will use them: the
//! macro expands at a call site outside this crate, where clippy runs with
//! warnings as errors, and the architecture document's examples build.

use std::collections::BTreeSet;
use std::error::Error;

use softshell_codec::json::{self, JsonErrorKind};
use softshell_codec::jsonc;
use softshell_codec::value::{self, Object, Value, value};
use softshell_core::clock::Timestamp;
use softshell_core::error::Chain;
use softshell_core::id::Id;
use softshell_core::limits::Limits;

struct Note {
    title: String,
    tags: Vec<String>,
    embedding: Vec<f32>,
}

#[test]
fn documents_build_with_every_key_form() {
    let note = Note {
        title: "Tide tables".into(),
        tags: vec!["ocean".into(), "notes".into()],
        embedding: vec![0.25, 0.5],
    };
    let key = "count";
    let id = Id::from_u128(1);
    // Parenthesized keys, references, method calls and casts, as a
    // caller writes them: none may warn.
    let doc = value!({
        "_id": (Value::Id(id)),
        "title": &note.title,
        "tags": (note.tags.iter().map(|t| Value::from(t.as_str())).collect::<Vec<_>>()),
        "tag_count": note.tags.len() as i64,
        (format!("{key}_plus_one")): 1 + 1,
        key: 1,
        (key.to_uppercase()): null,
        "embedding": note.embedding,
        "when": (Timestamp::from_nanos(0)),
        "nested": { "deep": [[], {}, -1, "x"] },
    });
    let object = doc.as_object().unwrap();
    assert_eq!(
        object.keys().collect::<Vec<_>>(),
        [
            "_id",
            "title",
            "tags",
            "tag_count",
            "count_plus_one",
            "count",
            "COUNT",
            "embedding",
            "when",
            "nested"
        ]
    );
    assert!(
        object
            .get("embedding")
            .unwrap()
            .identical(&Value::F32Vec(vec![0.25, 0.5]))
    );
    assert!(object.get("tag_count").unwrap().identical(&Value::Int(2)));

    // Through extended JSON and back.
    let text = json::to_string_pretty(&doc);
    let back = json::parse(&text, Limits::default()).unwrap();
    assert!(back.identical(&doc), "{text}");
    assert_eq!(back.to_string(), json::to_string_compact(&doc));
}

fn embedding_from_somewhere() -> Vec<f32> {
    vec![0.25, 0.5]
}

#[test]
fn the_architecture_examples_build() {
    // docs/01-architecture.md, as written there.
    let note = value!({
        "title": "Tide tables",
        "tags": ["ocean", "notes"],
        "body": "Low tide is the best time to explore tide pools.",
        "embedding": embedding_from_somewhere(),
    });
    assert_eq!(note.as_object().unwrap().len(), 4);
    assert!(value!({}).as_object().unwrap().is_empty());
    let count = value!({ "count": 1 });
    assert!(
        count
            .as_object()
            .unwrap()
            .get("count")
            .unwrap()
            .identical(&Value::Int(1))
    );
    let query = value!({ "tags": { "$in": ["ocean"] } });
    assert_eq!(
        json::to_string_compact(&query),
        r#"{"tags":{"$in":["ocean"]}}"#
    );
}

#[test]
fn values_work_as_ordered_set_members() {
    let set: BTreeSet<Value> = [
        Value::Int(5),
        Value::Float(5.0),
        Value::Float(f64::NAN),
        Value::Float(-f64::NAN),
        Value::from("a"),
        Value::Null,
    ]
    .into_iter()
    .collect();
    // 5 and 5.0 are one member, and so are the NaNs.
    assert_eq!(set.len(), 4);
    assert!(set.first().unwrap().is_null());
}

#[test]
fn objects_and_errors_from_outside() {
    let mut object = Object::new();
    object.insert("a", 1);
    let err = Object::try_from(vec![
        ("k".to_owned(), Value::Null),
        ("k".to_owned(), Value::Null),
    ])
    .unwrap_err();
    assert_eq!((err.key(), err.index()), ("k", 1));
    let _: &dyn Error = &err;

    let text = "{\n  // a config\n  \"dir\": \"x\",\n  \"dir\": \"y\",\n}";
    let err = jsonc::parse(text, Limits::default()).unwrap_err();
    assert!(matches!(err.kind(), JsonErrorKind::DuplicateKey { key } if key == "dir"));
    assert_eq!((err.line(), err.column(), err.format()), (4, 3, "JSONC"));
    assert_eq!(
        Chain::new(&err).to_string(),
        "JSONC at line 4, column 3: a key repeats an earlier key of the same object"
    );
    let err = json::parse(r#"{"$ts": "1600-01-01T00:00:00Z"}"#, Limits::default()).unwrap_err();
    assert!(err.source().is_some());
    let _ = value::Value::Null;
}
