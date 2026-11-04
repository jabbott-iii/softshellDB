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

//! [`Value`], the one value type that documents, graph properties, series
//! fields, query documents and protocol messages carry, and [`Object`], its
//! insertion-ordered map.
//!
//! Values have one total order across types (ADR-0013), which `Ord`, `Eq`
//! and `Hash` all follow:
//!
//! `Null < Bool < numbers < String < Bytes < Id < Timestamp < Array <
//! Object < F32Vec`
//!
//! - `Int` and `Float` share one numeric order, exact for every pair, so
//!   `Int(5) == Float(5.0)` and `Int(i64::MAX) < Float(2^63)`.
//! - NaN equals NaN and sorts after every other number; `-0.0` equals
//!   `0.0`. `F32Vec` elements compare the same way.
//! - Strings and bytes compare bytewise (no locale collation); arrays
//!   lexicographically by element; objects lexicographically by `(key,
//!   value)` pairs in their insertion order. A shorter prefix sorts first.
//!
//! The order is an on-disk contract: keycode and every index depend on it.
//! To tell `Int(5)` from `Float(5.0)`, or `-0.0` from `0.0`, use
//! [`Value::identical`].
//!
//! ```
//! use softshell_codec::value::Value;
//!
//! assert_eq!(Value::Int(5), Value::Float(5.0));
//! assert!(!Value::Int(5).identical(&Value::Float(5.0)));
//! assert!(Value::Int(i64::MAX) < Value::Float(9_223_372_036_854_775_808.0));
//! assert!(Value::Float(f64::INFINITY) < Value::Float(f64::NAN));
//! assert!(Value::Float(f64::NAN) < Value::String(String::new()));
//! ```
//!
//! NaN is not data (ADR-0022): every NaN is the same value, whatever its
//! sign and payload bits. JSON writes it as the name `NaN`, which reads
//! back as the canonical quiet NaN, and SDOC will store only that NaN.

// Objects from hostile input pass through the duplicate-key check here.
#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::hash::{Hash, Hasher};

use softshell_core::clock::Timestamp;
use softshell_core::id::Id;

#[doc(inline)]
pub use crate::__value as value;

/// 2^63, the first `f64` above every `i64`.
const TWO_POW_63: f64 = 9_223_372_036_854_775_808.0;

/// Objects with at most this many entries are checked for duplicate keys
/// pair by pair; larger ones are sorted.
const PAIRWISE_MAX: usize = 8;

/// A value of any type softshellDB stores.
///
/// `PartialEq`, `Eq`, `Ord` and `Hash` follow the value order in the
/// [module documentation](self), not the variant: `Int(5) == Float(5.0)`.
#[derive(Clone, Debug, Default)]
pub enum Value {
    /// No value.
    #[default]
    Null,
    /// `true` or `false`; `false` sorts first.
    Bool(bool),
    /// A 64-bit signed integer.
    Int(i64),
    /// A 64-bit float, possibly NaN or infinite.
    Float(f64),
    /// UTF-8 text.
    String(String),
    /// A byte string.
    Bytes(Vec<u8>),
    /// Nanoseconds since the Unix epoch, UTC (1677 to 2262, ADR-0013).
    Timestamp(Timestamp),
    /// A 128-bit id.
    Id(Id),
    /// An ordered list of values.
    Array(Vec<Value>),
    /// An insertion-ordered map with no repeated key.
    Object(Object),
    /// A dense vector of `f32`s, for vector search.
    F32Vec(Vec<f32>),
}

impl Value {
    /// The type's name, for messages: `"null"`, `"bool"`, `"int"`,
    /// `"float"`, `"string"`, `"bytes"`, `"timestamp"`, `"id"`, `"array"`,
    /// `"object"` or `"f32vec"`.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::Bytes(_) => "bytes",
            Self::Timestamp(_) => "timestamp",
            Self::Id(_) => "id",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
            Self::F32Vec(_) => "f32vec",
        }
    }

    /// Whether this is `Null`.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// The bool, if this is a `Bool`.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match *self {
            Self::Bool(b) => Some(b),
            _ => None,
        }
    }

    /// The integer, if this is an `Int`. A `Float` gives `None`, even
    /// `5.0`.
    #[must_use]
    pub const fn as_i64(&self) -> Option<i64> {
        match *self {
            Self::Int(i) => Some(i),
            _ => None,
        }
    }

    /// The float, if this is a `Float`. An `Int` gives `None`: converting
    /// one to `f64` can round.
    #[must_use]
    pub const fn as_f64(&self) -> Option<f64> {
        match *self {
            Self::Float(f) => Some(f),
            _ => None,
        }
    }

    /// The text, if this is a `String`.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// The bytes, if this is `Bytes`.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(b) => Some(b),
            _ => None,
        }
    }

    /// The timestamp, if this is a `Timestamp`.
    #[must_use]
    pub const fn as_timestamp(&self) -> Option<Timestamp> {
        match *self {
            Self::Timestamp(t) => Some(t),
            _ => None,
        }
    }

    /// The id, if this is an `Id`.
    #[must_use]
    pub const fn as_id(&self) -> Option<Id> {
        match *self {
            Self::Id(id) => Some(id),
            _ => None,
        }
    }

    /// The elements, if this is an `Array`.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The elements, mutably, if this is an `Array`.
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Self::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The object, if this is an `Object`.
    #[must_use]
    pub const fn as_object(&self) -> Option<&Object> {
        match self {
            Self::Object(o) => Some(o),
            _ => None,
        }
    }

    /// The object, mutably, if this is an `Object`.
    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Self::Object(o) => Some(o),
            _ => None,
        }
    }

    /// The elements, if this is an `F32Vec`.
    #[must_use]
    pub fn as_f32_vec(&self) -> Option<&[f32]> {
        match self {
            Self::F32Vec(v) => Some(v),
            _ => None,
        }
    }

    /// Whether `self` and `other` are the same variant with the same
    /// contents, bit for bit, except that every NaN is identical to every
    /// other NaN (NaN bits are not data, ADR-0022). Stricter than `==`:
    /// `Int(5)` is not identical to `Float(5.0)`, nor `Float(-0.0)` to
    /// `Float(0.0)`. Round-trip tests compare with this.
    #[must_use]
    pub fn identical(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => {
                a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
            }
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::Timestamp(a), Self::Timestamp(b)) => a == b,
            (Self::Id(a), Self::Id(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.identical(y))
            }
            (Self::Object(a), Self::Object(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .zip(b.iter())
                        .all(|((ka, va), (kb, vb))| ka == kb && va.identical(vb))
            }
            (Self::F32Vec(a), Self::F32Vec(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .zip(b)
                        .all(|(x, y)| x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()))
            }
            _ => false,
        }
    }

    /// The position of this value's type in the order; `Int` and `Float`
    /// share one.
    const fn rank(&self) -> u8 {
        match self {
            Self::Null => 0,
            Self::Bool(_) => 1,
            Self::Int(_) | Self::Float(_) => 2,
            Self::String(_) => 3,
            Self::Bytes(_) => 4,
            Self::Id(_) => 5,
            Self::Timestamp(_) => 6,
            Self::Array(_) => 7,
            Self::Object(_) => 8,
            Self::F32Vec(_) => 9,
        }
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (Self::Int(a), Self::Int(b)) => a.cmp(b),
            (Self::Int(a), Self::Float(b)) => cmp_int_float(*a, *b),
            (Self::Float(a), Self::Int(b)) => cmp_int_float(*b, *a).reverse(),
            (Self::Float(a), Self::Float(b)) => cmp_f64(*a, *b),
            (Self::String(a), Self::String(b)) => a.cmp(b),
            (Self::Bytes(a), Self::Bytes(b)) => a.cmp(b),
            (Self::Id(a), Self::Id(b)) => a.cmp(b),
            (Self::Timestamp(a), Self::Timestamp(b)) => a.cmp(b),
            (Self::Array(a), Self::Array(b)) => a.cmp(b),
            (Self::Object(a), Self::Object(b)) => a.cmp(b),
            (Self::F32Vec(a), Self::F32Vec(b)) => a
                .iter()
                .zip(b)
                .map(|(x, y)| cmp_f32(*x, *y))
                .find(|o| o.is_ne())
                .unwrap_or_else(|| a.len().cmp(&b.len())),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Value {}

impl Hash for Value {
    /// Equal values hash alike: a number equal to an `i64` hashes as that
    /// integer, whichever variant holds it.
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u8(self.rank());
        match self {
            Self::Null => {}
            Self::Bool(b) => b.hash(state),
            Self::Int(i) => hash_integer(*i, state),
            Self::Float(f) => hash_f64(*f, state),
            Self::String(s) => s.hash(state),
            Self::Bytes(b) => b.hash(state),
            Self::Timestamp(t) => t.hash(state),
            Self::Id(id) => id.hash(state),
            Self::Array(a) => a.hash(state),
            Self::Object(o) => o.hash(state),
            Self::F32Vec(v) => {
                state.write_usize(v.len());
                for &x in v {
                    state.write_u32(canonical_f32_bits(x));
                }
            }
        }
    }
}

/// `i` against `f`, exactly.
fn cmp_int_float(i: i64, f: f64) -> Ordering {
    if f.is_nan() {
        return Ordering::Less;
    }
    if f >= TWO_POW_63 {
        return Ordering::Less;
    }
    if f < -TWO_POW_63 {
        return Ordering::Greater;
    }
    // In [-2^63, 2^63), the integer part is exactly an i64.
    let whole = f.trunc();
    match i.cmp(&(whole as i64)) {
        // The fraction decides: `f - whole` is exact.
        Ordering::Equal => cmp_f64(whole, f),
        unequal => unequal,
    }
}

/// The value order on `f64`: NaN equals NaN and is the greatest; `-0.0`
/// equals `0.0`.
fn cmp_f64(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            if a < b {
                Ordering::Less
            } else if a > b {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        }
    }
}

/// [`cmp_f64`] for `f32`.
fn cmp_f32(a: f32, b: f32) -> Ordering {
    cmp_f64(f64::from(a), f64::from(b))
}

/// Numbers hash in one of three forms: an integer (tag 0), any other
/// non-NaN float by its bits (tag 1), or NaN (tag 2).
fn hash_integer<H: Hasher>(i: i64, state: &mut H) {
    state.write_u8(0);
    state.write_i64(i);
}

fn hash_f64<H: Hasher>(f: f64, state: &mut H) {
    if f.is_nan() {
        state.write_u8(2);
    } else if f.trunc() == f && (-TWO_POW_63..TWO_POW_63).contains(&f) {
        // Exactly an i64 (-0.0 included, as 0), so equal to that `Int`.
        hash_integer(f as i64, state);
    } else {
        state.write_u8(1);
        state.write_u64(f.to_bits());
    }
}

/// One bit pattern per class of equal `f32`s.
fn canonical_f32_bits(x: f32) -> u32 {
    if x.is_nan() {
        f32::NAN.to_bits()
    } else if x == 0.0 {
        0
    } else {
        x.to_bits()
    }
}

/// An insertion-ordered map from strings to values, with no repeated key.
///
/// Lookups scan the entries, which suits documents' small objects:
/// [`get`](Self::get), [`insert`](Self::insert) and
/// [`remove`](Self::remove) take time linear in the length. Building a
/// large object one [`insert`](Self::insert) at a time is quadratic; build
/// it from a `Vec` with [`Object::try_from`] instead, which checks for
/// duplicate keys in `O(n log n)`.
///
/// `==`, `Ord` and `Hash` follow the value order: entries compare in
/// insertion order, so `{"a": 1, "b": 2}` is not equal to `{"b": 2, "a": 1}`.
#[derive(Clone, Default)]
pub struct Object {
    entries: Vec<(String, Value)>,
}

impl Object {
    /// An empty object.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// An empty object with room for `capacity` entries.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
        }
    }

    /// The number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The value for `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The value for `key`, mutably.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.entries
            .iter_mut()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Whether `key` has an entry.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Sets `key` to `value`. A new key goes at the end; an existing one
    /// keeps its position, and its old value is returned.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) -> Option<Value> {
        let key = key.into();
        let value = value.into();
        match self.get_mut(&key) {
            Some(slot) => Some(std::mem::replace(slot, value)),
            None => {
                self.entries.push((key, value));
                None
            }
        }
    }

    /// Removes `key`'s entry and returns its value. The other entries keep
    /// their order.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(index).1)
    }

    /// The entries in insertion order.
    #[must_use]
    pub fn iter(&self) -> Iter<'_> {
        Iter(self.entries.iter())
    }

    /// The keys in insertion order.
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    /// The values in insertion order.
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &Value> + ExactSizeIterator {
        self.entries.iter().map(|(_, v)| v)
    }

    /// The values in insertion order, mutably.
    pub fn values_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = &mut Value> + ExactSizeIterator {
        self.entries.iter_mut().map(|(_, v)| v)
    }
}

impl Object {
    /// An object of `entries`, which the caller has checked for repeated
    /// keys with [`first_duplicate`].
    pub(crate) const fn from_unique(entries: Vec<(String, Value)>) -> Self {
        Self { entries }
    }
}

/// The index of the first entry whose key an earlier entry already has.
pub(crate) fn first_duplicate(entries: &[(String, Value)]) -> Option<usize> {
    if entries.len() <= PAIRWISE_MAX {
        return entries.iter().enumerate().find_map(|(later, (key, _))| {
            entries
                .iter()
                .take(later)
                .any(|(k, _)| k == key)
                .then_some(later)
        });
    }
    let key = |i: usize| entries.get(i).map(|(k, _)| k.as_str());
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_unstable_by(|&a, &b| key(a).cmp(&key(b)).then(a.cmp(&b)));
    // Sorted by key, then position: the second of each run of equal keys
    // is that key's first repeat.
    order
        .windows(2)
        .filter_map(|pair| match *pair {
            [a, b] if key(a) == key(b) => Some(b),
            _ => None,
        })
        .min()
}

impl TryFrom<Vec<(String, Value)>> for Object {
    type Error = DuplicateKeyError;

    /// The entries in their order, or an error naming the first entry
    /// whose key an earlier entry already has.
    fn try_from(entries: Vec<(String, Value)>) -> Result<Self, DuplicateKeyError> {
        match first_duplicate(&entries) {
            None => Ok(Self { entries }),
            Some(index) => {
                let key = entries
                    .into_iter()
                    .nth(index)
                    .map(|(k, _)| k)
                    .unwrap_or_default();
                Err(DuplicateKeyError { key, index })
            }
        }
    }
}

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl Ord for Object {
    /// By `(key, value)` pairs in insertion order, then by length.
    fn cmp(&self, other: &Self) -> Ordering {
        self.iter().cmp(other.iter())
    }
}

impl PartialOrd for Object {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Object {}

impl Hash for Object {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(self.len());
        for (key, value) in self {
            key.hash(state);
            value.hash(state);
        }
    }
}

/// An [`Object`]'s entries in insertion order, as `(key, value)` pairs.
#[derive(Clone, Debug)]
pub struct Iter<'a>(std::slice::Iter<'a, (String, Value)>);

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a str, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(k, v)| (k.as_str(), v))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for Iter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back().map(|(k, v)| (k.as_str(), v))
    }
}

impl ExactSizeIterator for Iter<'_> {}

impl<'a> IntoIterator for &'a Object {
    type Item = (&'a str, &'a Value);
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

impl IntoIterator for Object {
    type Item = (String, Value);
    type IntoIter = IntoIter;

    fn into_iter(self) -> IntoIter {
        IntoIter(self.entries.into_iter())
    }
}

/// An [`Object`]'s entries in insertion order, by value.
#[derive(Clone, Debug)]
pub struct IntoIter(std::vec::IntoIter<(String, Value)>);

impl Iterator for IntoIter {
    type Item = (String, Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for IntoIter {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back()
    }
}

impl ExactSizeIterator for IntoIter {}

/// [`Object::try_from`] found a key twice. Its `Display` does not include
/// the key, which may be user data; [`key`](Self::key) returns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateKeyError {
    key: String,
    index: usize,
}

impl DuplicateKeyError {
    /// The repeated key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The index of the entry that repeats it.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }
}

impl fmt::Display for DuplicateKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "entry {} repeats an earlier entry's key", self.index)
    }
}

/// A leaf error: it has no source.
impl Error for DuplicateKeyError {}

macro_rules! from_via {
    ($variant:ident <- $($source:ty),+) => {
        $(
            impl From<$source> for Value {
                fn from(value: $source) -> Self {
                    Self::$variant(value.into())
                }
            }
        )+
    };
}

from_via!(Bool <- bool);
from_via!(Int <- i8, i16, i32, i64, u8, u16, u32);
from_via!(Float <- f32, f64);
// No `From<Vec<u8>>`: inference would turn any vector of integer
// literals into `Bytes`. Write `Value::Bytes(v)`.
from_via!(String <- String, &String, &str);
// A vector of `f32`s is a dense vector, as embeddings are; a vector of
// float literals infers as one too. JSON-like `[0.1, 0.2]` in `value!`
// is an `Array` of `Float`s.
from_via!(F32Vec <- Vec<f32>, &[f32]);
from_via!(Array <- Vec<Value>);
from_via!(Object <- Object);
from_via!(Timestamp <- Timestamp);
from_via!(Id <- Id);

impl<T: Into<Value>> From<Option<T>> for Value {
    /// `None` is `Null`.
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Null, Into::into)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

    use std::hash::{Hash, Hasher};

    use softshell_core::hash::{FastHasher, fnv1a64};
    use softshell_core::rng::Pcg32;
    use softshell_core::testing::{check, check_sized};

    use super::*;
    use crate::test_values;

    const TWO_POW_53: i64 = 1 << 53;

    fn hash_of(v: &Value) -> u64 {
        let mut h = FastHasher::default();
        v.hash(&mut h);
        h.finish()
    }

    fn object(entries: &[(&str, Value)]) -> Object {
        let entries: Vec<(String, Value)> = entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect();
        Object::try_from(entries).unwrap()
    }

    #[test]
    fn types_sort_in_the_adr_order() {
        let id = Id::from_u128(0);
        let ascending = [
            Value::Null,
            Value::Bool(false),
            Value::Bool(true),
            Value::Float(f64::NEG_INFINITY),
            Value::Int(i64::MIN),
            Value::Float(-0.5),
            Value::Int(0),
            Value::Float(0.5),
            Value::Int(i64::MAX),
            Value::Float(f64::INFINITY),
            Value::Float(f64::NAN),
            Value::String(String::new()),
            Value::String("a".into()),
            Value::String("a\0".into()),
            Value::String("b".into()),
            Value::String("é".into()),
            Value::Bytes(vec![]),
            Value::Bytes(vec![0]),
            Value::Bytes(vec![255]),
            Value::Id(id),
            Value::Id(Id::from_u128(u128::MAX)),
            Value::Timestamp(Timestamp::MIN),
            Value::Timestamp(Timestamp::MAX),
            Value::Array(vec![]),
            Value::Array(vec![Value::Null]),
            Value::Array(vec![Value::Int(1)]),
            Value::Array(vec![Value::Int(1), Value::Null]),
            Value::Array(vec![Value::Int(2)]),
            Value::Object(Object::new()),
            // Pairs in order: the first value decides before the next key.
            Value::Object(object(&[("a", Value::Int(1)), ("c", Value::Null)])),
            Value::Object(object(&[("a", Value::Int(2))])),
            Value::Object(object(&[("a", Value::Int(2)), ("b", Value::Null)])),
            Value::Object(object(&[("b", Value::Int(1))])),
            Value::F32Vec(vec![]),
            Value::F32Vec(vec![f32::NEG_INFINITY]),
            Value::F32Vec(vec![0.0, 1.0]),
            Value::F32Vec(vec![0.0, f32::NAN]),
            Value::F32Vec(vec![1.0]),
        ];
        for (i, a) in ascending.iter().enumerate() {
            for (j, b) in ascending.iter().enumerate() {
                assert_eq!(a.cmp(b), i.cmp(&j), "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn equal_numbers_are_equal_whatever_their_variant() {
        let equal = [
            (Value::Int(5), Value::Float(5.0)),
            (Value::Int(0), Value::Float(-0.0)),
            (Value::Float(0.0), Value::Float(-0.0)),
            (Value::Float(f64::NAN), Value::Float(-f64::NAN)),
            (
                Value::Float(f64::NAN),
                Value::Float(f64::from_bits(0x7FF0_0000_0000_0001)),
            ),
            (
                Value::Int(TWO_POW_53),
                Value::Float(9_007_199_254_740_992.0),
            ),
            (
                Value::Int(i64::MIN),
                Value::Float(-9_223_372_036_854_775_808.0),
            ),
            (
                Value::Int(i64::MAX - 1023),
                Value::Float(9_223_372_036_854_774_784.0),
            ),
            (
                Value::F32Vec(vec![-0.0, f32::NAN]),
                Value::F32Vec(vec![0.0, -f32::NAN]),
            ),
            (
                Value::Array(vec![Value::Int(1)]),
                Value::Array(vec![Value::Float(1.0)]),
            ),
            (
                Value::Object(object(&[("a", Value::Int(1))])),
                Value::Object(object(&[("a", Value::Float(1.0))])),
            ),
        ];
        for (a, b) in &equal {
            assert_eq!(a, b);
            assert_eq!(b, a);
            assert_eq!(hash_of(a), hash_of(b), "{a:?} and {b:?}");
            assert!(!a.identical(b) || a.type_name() == b.type_name());
        }
        // The exact comparison near the ends of `f64`'s integers.
        let two_pow_53 = 9_007_199_254_740_992.0;
        assert!(Value::Int(TWO_POW_53 + 1) > Value::Float(two_pow_53));
        assert!(Value::Int(TWO_POW_53 + 1) < Value::Float(two_pow_53 + 2.0));
        assert!(Value::Int(-TWO_POW_53 - 1) < Value::Float(-two_pow_53));
        assert!(Value::Int(i64::MAX) < Value::Float(9_223_372_036_854_775_808.0));
        assert!(Value::Int(i64::MAX - 1023) == Value::Float(9_223_372_036_854_774_784.0));
        assert!(Value::Int(i64::MAX - 1022) > Value::Float(9_223_372_036_854_774_784.0));
        assert!(Value::Int(i64::MIN + 1) > Value::Float(-9_223_372_036_854_775_808.0));
        assert!(Value::Int(i64::MIN) > Value::Float(-9_223_372_036_854_777_856.0));
        assert!(Value::Int(3) < Value::Float(3.5));
        assert!(Value::Int(-3) > Value::Float(-3.5));
        assert!(Value::Int(-4) < Value::Float(-3.5));
        assert!(Value::Int(i64::MAX) < Value::Float(f64::NAN));
        // `i64::MAX as f64` rounds up to 2^63: a naive conversion would call
        // these equal.
        assert_ne!(Value::Int(i64::MAX), Value::Float(i64::MAX as f64));
        assert_ne!(
            Value::Int(TWO_POW_53 + 1),
            Value::Float((TWO_POW_53 + 1) as f64)
        );
    }

    /// Mirrored, call for call, by the Python reference that computed the
    /// pinned aggregate with Python's exact int/float comparison.
    #[test]
    fn int_float_order_matches_the_reference() {
        const INTS: [i64; 18] = [
            0,
            1,
            -1,
            TWO_POW_53 - 1,
            TWO_POW_53,
            TWO_POW_53 + 1,
            -TWO_POW_53 + 1,
            -TWO_POW_53,
            -TWO_POW_53 - 1,
            1 << 62,
            i64::MAX - 1024,
            i64::MAX - 1023,
            i64::MAX - 1022,
            i64::MAX,
            i64::MIN,
            i64::MIN + 1,
            i64::MIN + 1024,
            i64::MIN + 1025,
        ];
        const FLOATS: [f64; 26] = [
            0.0,
            -0.0,
            0.5,
            -0.5,
            1.0,
            -1.0,
            9_007_199_254_740_991.0,
            9_007_199_254_740_992.0,
            9_007_199_254_740_994.0,
            -9_007_199_254_740_992.0,
            -9_007_199_254_740_994.0,
            4_611_686_018_427_387_904.0,
            9_223_372_036_854_774_784.0,
            9_223_372_036_854_775_808.0,
            18_446_744_073_709_551_616.0,
            -9_223_372_036_854_774_784.0,
            -9_223_372_036_854_775_808.0,
            -9_223_372_036_854_777_856.0,
            1e300,
            -1e300,
            5e-324,
            -5e-324,
            // 2^53 + 0.5 rounds to 2^53.
            9_007_199_254_740_992.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ];
        let symbol = |i: i64, f: f64| match Value::Int(i).cmp(&Value::Float(f)) {
            Ordering::Less => '<',
            Ordering::Equal => '=',
            Ordering::Greater => '>',
        };
        let mut all = String::new();
        let mut lines = 0;
        let mut line = |i: i64, f: f64| {
            all.push_str(&format!("{i} {:016x} {}\n", f.to_bits(), symbol(i, f)));
            lines += 1;
        };
        for i in INTS {
            for f in FLOATS {
                line(i, f);
            }
        }
        let mut rng = Pcg32::new(0x5EED, 5);
        for _ in 0..20_000 {
            let (i, f) = match rng.next_u32() % 4 {
                0 => (rng.next_u64() as i64, f64::from_bits(rng.next_u64())),
                1 => {
                    let i = rng.next_u64() as i64;
                    (i, i as f64)
                }
                2 => {
                    let shift = rng.next_u32() % 64;
                    let mut i = (rng.next_u64() >> shift) as i64;
                    if rng.next_u32() & 1 == 1 {
                        i = i.wrapping_neg();
                    }
                    let mut bits = (i as f64).to_bits();
                    match rng.next_u32() % 3 {
                        0 if bits & !(1 << 63) != 0 => bits -= 1,
                        2 => bits += 1,
                        _ => {}
                    }
                    (i, f64::from_bits(bits))
                }
                _ => {
                    let i = INTS[(rng.next_u32() as usize) % INTS.len()];
                    (i, FLOATS[(rng.next_u32() as usize) % FLOATS.len()])
                }
            };
            line(i, f);
        }
        assert_eq!(
            (lines, fnv1a64(all.as_bytes())),
            (20_468, 0x80FC_B55D_2962_F539)
        );
    }

    fn order_problem(a: &Value, b: &Value) -> Option<String> {
        let (ab, ba) = (a.cmp(b), b.cmp(a));
        if ab != ba.reverse() {
            return Some(format!(
                "not antisymmetric: {a:?} {ab:?} {b:?}, but {ba:?} back"
            ));
        }
        if (a == b) != ab.is_eq() || a.partial_cmp(b) != Some(ab) {
            return Some(format!("==, partial_cmp and cmp disagree on {a:?}, {b:?}"));
        }
        if ab.is_eq() && hash_of(a) != hash_of(b) {
            return Some(format!("equal but hash differently: {a:?}, {b:?}"));
        }
        None
    }

    #[test]
    fn the_order_is_total() {
        let triple = |rng: &mut Pcg32, size: u32| {
            [
                test_values::narrow(rng, size),
                test_values::narrow(rng, size),
                test_values::narrow(rng, size),
            ]
        };
        check_sized(
            "value order is a total order",
            20_000,
            triple,
            |[a, b, c]| {
                for v in [a, b, c] {
                    if v.cmp(v) != Ordering::Equal || v != v {
                        return Err(format!("not reflexive: {v:?}"));
                    }
                }
                for (x, y) in [(a, b), (b, c), (a, c)] {
                    if let Some(problem) = order_problem(x, y) {
                        return Err(problem);
                    }
                }
                // Transitive, for every arrangement of the three.
                for [x, y, z] in [
                    [a, b, c],
                    [a, c, b],
                    [b, a, c],
                    [b, c, a],
                    [c, a, b],
                    [c, b, a],
                ] {
                    let (xy, yz, xz) = (x.cmp(y), y.cmp(z), x.cmp(z));
                    if xy.is_le() && yz.is_le() && !xz.is_le() {
                        return Err(format!("not transitive: {x:?} <= {y:?} <= {z:?}"));
                    }
                    if xy.is_eq() && yz.is_eq() && !xz.is_eq() {
                        return Err(format!("equality not transitive: {x:?} {y:?} {z:?}"));
                    }
                }
                Ok(())
            },
        );
        check_sized(
            "wide values compare consistently",
            3000,
            |rng, size| (test_values::wide(rng, size), test_values::wide(rng, size)),
            |(a, b)| match order_problem(a, b) {
                Some(problem) => Err(problem),
                None if a.cmp(a).is_eq() && a.clone().identical(a) => Ok(()),
                None => Err(format!("{a:?} does not equal its clone")),
            },
        );
    }

    #[test]
    fn numbers_compare_like_exact_rationals() {
        // An independent exact comparison: both sides as i128 multiples of
        // 2^-1074 would not fit, so compare the integer parts in i128 and
        // then the fraction.
        fn exact(i: i64, f: f64) -> Ordering {
            if f.is_nan() {
                return Ordering::Less;
            }
            if f.is_infinite() || f.abs() >= 1e30 {
                return if f > 0.0 {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
            let floor = f.floor();
            match i128::from(i).cmp(&(floor as i128)) {
                Ordering::Equal if f > floor => Ordering::Less,
                other => other,
            }
        }
        check(
            "Int against Float is exact",
            100_000,
            |rng| (test_values::int(rng, false), test_values::float(rng, false)),
            |&(i, f)| {
                let found = Value::Int(i).cmp(&Value::Float(f));
                if found == exact(i, f) {
                    Ok(())
                } else {
                    Err(format!("{i} vs {f:e}: {found:?}"))
                }
            },
        );
    }

    #[test]
    fn identical_tells_variants_and_signed_zeros_apart() {
        assert!(Value::Float(f64::NAN).identical(&Value::Float(-f64::NAN)));
        assert!(!Value::Float(0.0).identical(&Value::Float(-0.0)));
        assert!(!Value::Int(1).identical(&Value::Float(1.0)));
        assert!(!Value::F32Vec(vec![0.0]).identical(&Value::F32Vec(vec![-0.0])));
        assert!(Value::F32Vec(vec![f32::NAN]).identical(&Value::F32Vec(vec![-f32::NAN])));
        assert!(!Value::F32Vec(vec![1.0]).identical(&Value::F32Vec(vec![1.0, 1.0])));
        let a = Value::Array(vec![Value::Object(object(&[("k", Value::Int(1))]))]);
        let b = Value::Array(vec![Value::Object(object(&[("k", Value::Float(1.0))]))]);
        assert!(a == b && !a.identical(&b));
        assert!(
            !Value::Object(object(&[("k", Value::Null)]))
                .identical(&Value::Object(object(&[("j", Value::Null)])))
        );
        assert!(!Value::Array(vec![]).identical(&Value::Array(vec![Value::Null])));
    }

    #[test]
    fn accessors_and_type_names() {
        let id = Id::from_u128(7);
        let ts = Timestamp::from_nanos(7);
        let cases: [(Value, &str); 11] = [
            (Value::Null, "null"),
            (Value::Bool(true), "bool"),
            (Value::Int(1), "int"),
            (Value::Float(1.0), "float"),
            (Value::from("s"), "string"),
            (Value::Bytes(vec![1]), "bytes"),
            (Value::Timestamp(ts), "timestamp"),
            (Value::Id(id), "id"),
            (Value::Array(vec![]), "array"),
            (Value::Object(Object::new()), "object"),
            (Value::F32Vec(vec![1.0]), "f32vec"),
        ];
        for (v, name) in &cases {
            assert_eq!(v.type_name(), *name);
            assert_eq!(v.is_null(), *name == "null");
            assert_eq!(v.as_bool().is_some(), *name == "bool");
            assert_eq!(v.as_i64().is_some(), *name == "int");
            assert_eq!(v.as_f64().is_some(), *name == "float");
            assert_eq!(v.as_str().is_some(), *name == "string");
            assert_eq!(v.as_bytes().is_some(), *name == "bytes");
            assert_eq!(v.as_timestamp().is_some(), *name == "timestamp");
            assert_eq!(v.as_id().is_some(), *name == "id");
            assert_eq!(v.as_array().is_some(), *name == "array");
            assert_eq!(v.as_object().is_some(), *name == "object");
            assert_eq!(v.as_f32_vec().is_some(), *name == "f32vec");
        }
        assert_eq!(Value::Int(5).as_f64(), None);
        assert_eq!(Value::Float(5.0).as_i64(), None);
        assert_eq!(Value::Id(id).as_id(), Some(id));
        assert_eq!(Value::Timestamp(ts).as_timestamp(), Some(ts));
        let mut v = Value::Array(vec![]);
        v.as_array_mut().unwrap().push(Value::Null);
        assert_eq!(v.as_array().unwrap().len(), 1);
        let mut v = Value::Object(Object::new());
        v.as_object_mut().unwrap().insert("k", 1);
        assert!(
            v.as_object()
                .unwrap()
                .get("k")
                .unwrap()
                .identical(&Value::Int(1))
        );
        assert!(Value::default().is_null());
    }

    #[test]
    fn conversions_pick_the_variant() {
        let s = String::from("s");
        let cases: Vec<(Value, Value)> = vec![
            (Value::from(true), Value::Bool(true)),
            (Value::from(-1i8), Value::Int(-1)),
            (Value::from(-1i16), Value::Int(-1)),
            (Value::from(-1i32), Value::Int(-1)),
            (Value::from(i64::MIN), Value::Int(i64::MIN)),
            (Value::from(255u8), Value::Int(255)),
            (Value::from(65_535u16), Value::Int(65_535)),
            (Value::from(u32::MAX), Value::Int(4_294_967_295)),
            (Value::from(0.1f32), Value::Float(f64::from(0.1f32))),
            (Value::from(0.1), Value::Float(0.1)),
            (Value::from(&s), Value::String("s".into())),
            (Value::from(s.clone()), Value::String("s".into())),
            (Value::from("t"), Value::String("t".into())),
            (Value::from(vec![1.5f32]), Value::F32Vec(vec![1.5])),
            (Value::from(&[1.5f32][..]), Value::F32Vec(vec![1.5])),
            (
                Value::from(vec![Value::Null]),
                Value::Array(vec![Value::Null]),
            ),
            (Value::from(Object::new()), Value::Object(Object::new())),
            (
                Value::from(Timestamp::MAX),
                Value::Timestamp(Timestamp::MAX),
            ),
            (Value::from(Id::from_u128(1)), Value::Id(Id::from_u128(1))),
            (Value::from(None::<bool>), Value::Null),
            (Value::from(Some("x")), Value::String("x".into())),
        ];
        for (found, expected) in &cases {
            assert!(found.identical(expected), "{found:?} is not {expected:?}");
        }
    }

    #[test]
    fn objects_keep_insertion_order_and_unique_keys() {
        let mut o = Object::with_capacity(4);
        assert!(o.is_empty());
        assert_eq!(o.insert("b", 1), None);
        assert_eq!(o.insert(String::from("a"), 2), None);
        assert_eq!(o.insert("c", 3), None);
        // A repeated key keeps its place.
        assert!(o.insert("b", "x").unwrap().identical(&Value::Int(1)));
        assert_eq!(o.keys().collect::<Vec<_>>(), ["b", "a", "c"]);
        assert_eq!(o.len(), 3);
        assert!(o.contains_key("a") && !o.contains_key("z"));
        *o.get_mut("a").unwrap() = Value::Null;
        assert!(o.get("a").unwrap().is_null());
        assert_eq!(o.get("z"), None);
        assert!(o.remove("b").unwrap().identical(&Value::from("x")));
        assert_eq!(o.remove("b"), None);
        assert_eq!(o.keys().collect::<Vec<_>>(), ["a", "c"]);
        for v in o.values_mut() {
            *v = Value::Bool(true);
        }
        assert!(o.values().all(|v| v.identical(&Value::Bool(true))));
        let pairs: Vec<(&str, &Value)> = (&o).into_iter().collect();
        assert_eq!(pairs.len(), 2);
        assert_eq!(o.iter().len(), 2);
        assert_eq!(o.iter().next_back().unwrap().0, "c");
        let mut owned = o.clone().into_iter();
        assert_eq!(owned.len(), 2);
        assert_eq!(owned.next_back().unwrap().0, "c");
        assert_eq!(owned.next().unwrap().0, "a");
        assert_eq!(format!("{o:?}"), r#"{"a": Bool(true), "c": Bool(true)}"#);
        // Objects compare by their entries in order.
        let ab = object(&[("a", Value::Int(1)), ("b", Value::Int(2))]);
        let ba = object(&[("b", Value::Int(2)), ("a", Value::Int(1))]);
        assert_ne!(ab, ba);
        assert!(ab < ba);
        assert_eq!(
            ab,
            object(&[("a", Value::Float(1.0)), ("b", Value::Int(2))])
        );
    }

    #[test]
    fn duplicate_keys_are_found_in_order() {
        let entries = |keys: &[&str]| -> Vec<(String, Value)> {
            keys.iter()
                .map(|k| ((*k).to_owned(), Value::Null))
                .collect()
        };
        let err = Object::try_from(entries(&["a", "b", "a"])).unwrap_err();
        assert_eq!((err.key(), err.index()), ("a", 2));
        assert_eq!(err.to_string(), "entry 2 repeats an earlier entry's key");
        // Larger objects are sorted; the first repeat in order is reported.
        let mut keys: Vec<String> = (0..30).map(|i| format!("k{i}")).collect();
        keys.extend(["k20".into(), "k3".into(), "k20".into()]);
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        let err = Object::try_from(entries(&refs)).unwrap_err();
        assert_eq!((err.key(), err.index()), ("k20", 30));
        check(
            "first_duplicate matches a quadratic search",
            2000,
            |rng| {
                let n = rng.range(0..40) as usize;
                let pool = rng.range(1..60);
                (0..n)
                    .map(|_| format!("{}", rng.range(0..pool)))
                    .collect::<Vec<_>>()
            },
            |keys| {
                let expected = (0..keys.len()).find(|&j| keys[..j].contains(&keys[j]));
                let entries: Vec<(String, Value)> =
                    keys.iter().map(|k| (k.clone(), Value::Null)).collect();
                let found = first_duplicate(&entries);
                if found == expected {
                    Ok(())
                } else {
                    Err(format!("found {found:?}, expected {expected:?}"))
                }
            },
        );
        assert!(Object::try_from(entries(&["a", "b"])).is_ok());
    }
}
