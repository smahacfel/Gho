use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/// Error returned by the normative RFC 8785 + SHA-256 hash contract.
#[derive(Debug, Error)]
pub enum CanonicalHashErrorV1 {
    #[error("RFC 8785 JCS serialization failed: {0}")]
    JcsSerialization(#[from] serde_json::Error),
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

/// Produce the exact UTF-8 RFC 8785 JCS bytes used as canonical hash input.
///
/// The selected implementation rejects NaN and infinities, performs UTF-16
/// property ordering required by JCS, and emits neither BOM nor trailing LF.
pub fn canonical_jcs_bytes_v1<T: Serialize>(payload: &T) -> Result<Vec<u8>, CanonicalHashErrorV1> {
    Ok(serde_json_canonicalizer::to_vec(payload)?)
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
