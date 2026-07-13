use serde::ser::{
    SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::fmt;
use std::io;
use std::str::FromStr;
use thiserror::Error;

/// Error returned by the normative RFC 8785 + SHA-256 hash contract.
#[derive(Debug, Error)]
pub enum CanonicalHashErrorV1 {
    #[error("RFC 8785 JCS serialization failed: {0}")]
    JcsSerialization(#[from] serde_json::Error),
    #[error("RFC 8785 JCS input validation failed: {0}")]
    JcsInputValidation(String),
    #[error("canonical SHA-256 digest must be exactly 64 lowercase hexadecimal characters")]
    InvalidDigest,
}

/// SHA-256 over RFC 8785 JCS bytes, encoded as 64 lowercase hexadecimal chars.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalHashV1(String);

impl CanonicalHashV1 {
    /// Hash a schema-defined semantic payload using RFC 8785 JCS and SHA-256.
    pub fn digest<T: Serialize>(payload: &T) -> Result<Self, CanonicalHashErrorV1> {
        let bytes = canonical_jcs_bytes_v1(payload)?;
        let digest = Sha256::digest(bytes);
        Ok(Self(format!("{digest:x}")))
    }

    /// Parse and validate an externally supplied canonical digest.
    pub fn parse(value: impl Into<String>) -> Result<Self, CanonicalHashErrorV1> {
        let value = value.into();
        if value.len() != 64
            || !value
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            return Err(CanonicalHashErrorV1::InvalidDigest);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for CanonicalHashV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for CanonicalHashV1 {
    type Err = CanonicalHashErrorV1;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for CanonicalHashV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CanonicalHashV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(de::Error::custom)
    }
}

#[derive(Debug)]
struct JcsInputValidationError(String);

impl fmt::Display for JcsInputValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for JcsInputValidationError {}

impl serde::ser::Error for JcsInputValidationError {
    fn custom<T>(message: T) -> Self
    where
        T: fmt::Display,
    {
        Self(message.to_string())
    }
}

/// Allocation-free serde walk used only to retain the established fail-closed
/// handling of NaN and infinities. `serde_json` itself renders those values as
/// `null`, which would erase the distinction before the optimized JCS pass.
#[derive(Clone, Copy)]
struct FiniteNumberValidator<'a> {
    contains_wide_integer: &'a Cell<bool>,
}

impl Serializer for FiniteNumberValidator<'_> {
    type Ok = ();
    type Error = JcsInputValidationError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, _value: bool) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_i8(self, _value: i8) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_i16(self, _value: i16) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_i32(self, _value: i32) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_i64(self, _value: i64) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_i128(self, value: i128) -> Result<(), Self::Error> {
        if i64::try_from(value).is_err() {
            self.contains_wide_integer.set(true);
        }
        Ok(())
    }
    fn serialize_u8(self, _value: u8) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_u16(self, _value: u16) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_u32(self, _value: u32) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_u64(self, _value: u64) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_u128(self, value: u128) -> Result<(), Self::Error> {
        if u64::try_from(value).is_err() {
            self.contains_wide_integer.set(true);
        }
        Ok(())
    }
    fn serialize_f32(self, value: f32) -> Result<(), Self::Error> {
        if value.is_finite() {
            Ok(())
        } else {
            Err(JcsInputValidationError(
                "NaN and +/-Infinity are not permitted in JCS".to_string(),
            ))
        }
    }
    fn serialize_f64(self, value: f64) -> Result<(), Self::Error> {
        if value.is_finite() {
            Ok(())
        } else {
            Err(JcsInputValidationError(
                "NaN and +/-Infinity are not permitted in JCS".to_string(),
            ))
        }
    }
    fn serialize_char(self, _value: char) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_str(self, _value: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_bytes(self, _value: &[u8]) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_none(self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_some<T>(self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(self)
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(self)
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(self)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(self)
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(self)
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(self)
    }
    fn collect_str<T>(self, _value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + fmt::Display,
    {
        Ok(())
    }
}

impl SerializeSeq for FiniteNumberValidator<'_> {
    type Ok = ();
    type Error = JcsInputValidationError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(*self)
    }

    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl SerializeTuple for FiniteNumberValidator<'_> {
    type Ok = ();
    type Error = JcsInputValidationError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(*self)
    }

    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl SerializeTupleStruct for FiniteNumberValidator<'_> {
    type Ok = ();
    type Error = JcsInputValidationError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(*self)
    }

    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl SerializeTupleVariant for FiniteNumberValidator<'_> {
    type Ok = ();
    type Error = JcsInputValidationError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(*self)
    }

    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl SerializeMap for FiniteNumberValidator<'_> {
    type Ok = ();
    type Error = JcsInputValidationError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        key.serialize(*self)
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(*self)
    }

    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl SerializeStruct for FiniteNumberValidator<'_> {
    type Ok = ();
    type Error = JcsInputValidationError;

    fn serialize_field<T>(&mut self, _key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(*self)
    }

    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl SerializeStructVariant for FiniteNumberValidator<'_> {
    type Ok = ();
    type Error = JcsInputValidationError;

    fn serialize_field<T>(&mut self, _key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(*self)
    }

    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn write_canonical_json_value_v1(
    value: &serde_json::Value,
    output: &mut Vec<u8>,
) -> serde_json::Result<()> {
    match value {
        serde_json::Value::Null => output.extend_from_slice(b"null"),
        serde_json::Value::Bool(true) => output.extend_from_slice(b"true"),
        serde_json::Value::Bool(false) => output.extend_from_slice(b"false"),
        serde_json::Value::Number(number) => {
            let value = number.as_f64().ok_or_else(|| {
                serde_json::Error::io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "JSON number cannot be represented as an IEEE-754 double",
                ))
            })?;
            if !value.is_finite() {
                return Err(serde_json::Error::io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "NaN and +/-Infinity are not permitted in JCS",
                )));
            }
            let mut buffer = ryu_js::Buffer::new();
            output.extend_from_slice(buffer.format_finite(value).as_bytes());
        }
        serde_json::Value::String(value) => serde_json::to_writer(output, value)?,
        serde_json::Value::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                write_canonical_json_value_v1(value, output)?;
            }
            output.push(b']');
        }
        serde_json::Value::Object(object) => {
            // RFC 8785 sorts property names by their raw UTF-16 code units,
            // not by UTF-8 bytes. Precompute each key once so comparison does
            // not repeatedly allocate on large evidence/config payloads.
            let mut entries = object
                .iter()
                .map(|(key, value)| (key.encode_utf16().collect::<Vec<_>>(), key, value))
                .collect::<Vec<_>>();
            entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
            output.push(b'{');
            for (index, (_, key, value)) in entries.into_iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                serde_json::to_writer(&mut *output, key)?;
                output.push(b':');
                write_canonical_json_value_v1(value, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

/// Produce the exact UTF-8 RFC 8785 JCS bytes used as canonical hash input.
///
/// This implementation preserves the established canonicalizer semantics but
/// avoids its per-property nested byte buffers and reparsing of every object
/// key. The first pass rejects non-finite values explicitly; the second pass
/// emits JavaScript-compatible numbers and raw UTF-16 property ordering. Very
/// wide numeric inputs that the intermediate JSON value cannot represent fall
/// back to the established reference implementation, preserving the generic
/// public contract outside the metric-contract schemas (which encode wide
/// integers as canonical strings).
pub fn canonical_jcs_bytes_v1<T: Serialize>(payload: &T) -> Result<Vec<u8>, CanonicalHashErrorV1> {
    let contains_wide_integer = Cell::new(false);
    payload
        .serialize(FiniteNumberValidator {
            contains_wide_integer: &contains_wide_integer,
        })
        .map_err(|error| CanonicalHashErrorV1::JcsInputValidation(error.to_string()))?;
    if contains_wide_integer.get() {
        return Ok(serde_json_canonicalizer::to_vec(payload)?);
    }
    let mut non_finite_checked_json = Vec::with_capacity(1_024);
    serde_json::to_writer(&mut non_finite_checked_json, payload)?;
    let value = match serde_json::from_slice::<serde_json::Value>(&non_finite_checked_json) {
        Ok(value) => value,
        Err(_) => return Ok(serde_json_canonicalizer::to_vec(payload)?),
    };
    let mut canonical = Vec::with_capacity(non_finite_checked_json.len());
    write_canonical_json_value_v1(&value, &mut canonical)?;
    Ok(canonical)
}

/// Required nullable semantic field.
///
/// Unlike `Option<T>`, a struct field of this type is still required during
/// deserialization. It therefore preserves the PR1 distinction between an
/// omitted key and an explicitly present JSON `null`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum CanonicalNullableV1<T> {
    #[default]
    Null,
    Value(T),
}

impl<T> CanonicalNullableV1<T> {
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    #[must_use]
    pub fn as_ref(&self) -> CanonicalNullableV1<&T> {
        match self {
            Self::Null => CanonicalNullableV1::Null,
            Self::Value(value) => CanonicalNullableV1::Value(value),
        }
    }

    #[must_use]
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Null => None,
            Self::Value(value) => Some(value),
        }
    }
}

impl<T> From<Option<T>> for CanonicalNullableV1<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(value) => Self::Value(value),
            None => Self::Null,
        }
    }
}

impl<T: Serialize> Serialize for CanonicalNullableV1<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Value(value) => value.serialize(serializer),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for CanonicalNullableV1<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Do not delegate to `Option<T>` here. Serde deliberately lets an
        // `Option` deserialize from a missing struct field, which would erase
        // the contract-level distinction between omitted and explicit null.
        // `serde_json::Value` asks the format for an actual value: explicit
        // null becomes `Value::Null`, while Serde's missing-field deserializer
        // fails before this method can manufacture a default.
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.is_null() {
            return Ok(Self::Null);
        }
        T::deserialize(value)
            .map(Self::Value)
            .map_err(de::Error::custom)
    }
}

fn is_canonical_unsigned_decimal(value: &str) -> bool {
    if value == "0" {
        return true;
    }
    !value.is_empty() && !value.starts_with('0') && value.as_bytes().iter().all(u8::is_ascii_digit)
}

fn is_canonical_signed_decimal(value: &str) -> bool {
    if value == "0" {
        return true;
    }
    if let Some(unsigned) = value.strip_prefix('-') {
        return unsigned != "0" && is_canonical_unsigned_decimal(unsigned);
    }
    is_canonical_unsigned_decimal(value)
}

/// Schema-typed canonical decimal string for a wide `u64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalU64StringV1(u64);

impl CanonicalU64StringV1 {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Schema-typed canonical decimal string for a wide `u128`.
///
/// Raw pump.fun token deltas and their cumulative sums are represented as
/// `u128` in the active fingerprint path. Keeping that width in the durable
/// contract prevents a valid runtime value from being truncated merely to fit
/// an interoperable JSON number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalU128StringV1(u128);

impl CanonicalU128StringV1 {
    #[must_use]
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u128 {
        self.0
    }
}

impl Serialize for CanonicalU128StringV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CanonicalU128StringV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        if !is_canonical_unsigned_decimal(&raw) {
            return Err(de::Error::custom(
                "wide u128 must be a canonical base-10 string without sign or leading zeros",
            ));
        }
        raw.parse::<u128>()
            .map(Self)
            .map_err(|error| de::Error::custom(format!("wide u128 out of range: {error}")))
    }
}

impl Serialize for CanonicalU64StringV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CanonicalU64StringV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        if !is_canonical_unsigned_decimal(&raw) {
            return Err(de::Error::custom(
                "wide u64 must be a canonical base-10 string without sign or leading zeros",
            ));
        }
        raw.parse::<u64>()
            .map(Self)
            .map_err(|error| de::Error::custom(format!("wide u64 out of range: {error}")))
    }
}

/// Schema-typed canonical decimal string for a wide `i64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalI64StringV1(i64);

impl CanonicalI64StringV1 {
    #[must_use]
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl Serialize for CanonicalI64StringV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CanonicalI64StringV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        if !is_canonical_signed_decimal(&raw) {
            return Err(de::Error::custom(
                "wide i64 must be a canonical base-10 string without plus sign or leading zeros",
            ));
        }
        raw.parse::<i64>()
            .map(Self)
            .map_err(|error| de::Error::custom(format!("wide i64 out of range: {error}")))
    }
}
