//! Typed property-key identity shared by authored IR and semantic consumers.

use std::fmt;
use std::sync::Arc;

/// An integer whose decimal digits are its canonical ECMAScript number
/// spelling.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    verter_no_typeexpr::NoTypeExpr,
    verter_no_storedspan::NoStoredSpan,
)]
pub struct CanonicalIndexInt(i64);

impl CanonicalIndexInt {
    /// Admit an integer exactly when converting it through `f64` preserves both
    /// its value and its canonical ECMAScript spelling.
    #[must_use]
    pub fn from_canonical_i64(value: i64) -> Option<Self> {
        Self::from_js_number(value as f64).filter(|key| key.0 == value)
    }

    /// Admit a numeric literal exactly when its canonical ECMAScript spelling
    /// is the candidate integer's decimal spelling.
    #[must_use]
    pub fn from_js_number(number: f64) -> Option<Self> {
        let candidate = number as i64;
        (verter_ecma::js_number_to_string(number) == candidate.to_string())
            .then_some(Self(candidate))
    }

    /// Return the admitted integer.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// Serde input enters through the checked constructor: a wire value whose
/// digits are not its canonical ECMAScript spelling is a decode error, never
/// an admitted key.
impl<'de> serde::Deserialize<'de> for CanonicalIndexInt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = i64::deserialize(deserializer)?;
        Self::from_canonical_i64(value).ok_or_else(|| {
            serde::de::Error::custom(format_args!(
                "integer {value} is not its canonical ECMAScript spelling"
            ))
        })
    }
}

impl fmt::Display for CanonicalIndexInt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Canonical property-key identity. `I` is the nominal declaration identity
/// used by the owning layer.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    verter_no_typeexpr::NoTypeExpr,
    verter_no_storedspan::NoStoredSpan,
)]
pub enum PropertyKey<I> {
    String(Arc<str>),
    Number(CanonicalIndexInt),
    UniqueSymbol(I),
}

impl<I> PropertyKey<I> {
    /// Normalize an identifier spelling into the shared string-key variant.
    #[must_use]
    pub fn identifier(name: impl Into<Arc<str>>) -> Self {
        Self::String(name.into())
    }

    /// Normalize a string-literal spelling into the shared string-key variant.
    #[must_use]
    pub fn string_literal(value: impl Into<Arc<str>>) -> Self {
        Self::String(value.into())
    }

    /// Normalize a numeric property key through ECMAScript's canonical number
    /// spelling. Canonical integers retain the numeric variant; every other
    /// number becomes its exact property-string identity.
    #[must_use]
    pub fn from_js_number(value: f64) -> Self {
        CanonicalIndexInt::from_js_number(value)
            .map(Self::Number)
            .unwrap_or_else(|| Self::String(Arc::from(verter_ecma::js_number_to_string(value))))
    }

    /// Borrow the ordinary string spelling when this is a string key.
    #[must_use]
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            Self::Number(_) | Self::UniqueSymbol(_) => None,
        }
    }

    /// Borrow the nominal-identity payload without changing key identity.
    #[must_use]
    pub fn as_ref(&self) -> PropertyKey<&I> {
        match self {
            Self::String(value) => PropertyKey::String(Arc::clone(value)),
            Self::Number(value) => PropertyKey::Number(*value),
            Self::UniqueSymbol(identity) => PropertyKey::UniqueSymbol(identity),
        }
    }

    /// Translate the nominal-identity payload without changing key identity.
    pub fn map_identity<J>(self, map: impl FnOnce(I) -> J) -> PropertyKey<J> {
        match self {
            Self::String(value) => PropertyKey::String(value),
            Self::Number(value) => PropertyKey::Number(value),
            Self::UniqueSymbol(identity) => PropertyKey::UniqueSymbol(map(identity)),
        }
    }

    /// The element-access equivalent under JS property identity: a numeric
    /// key coerces to its canonical string spelling, and a string whose
    /// spelling is exactly the canonical JS number form of an admissible
    /// integer coerces to the numeric variant. Only member LOOKUP coerces —
    /// `keyof` and domain enumeration keep the authored variant.
    #[must_use]
    pub fn element_access_equivalent(&self) -> Option<Self> {
        match self {
            Self::Number(value) => Some(Self::String(Arc::from(value.get().to_string()))),
            Self::String(value) => {
                let parsed: i64 = value.parse().ok()?;
                if parsed.to_string() != value.as_ref() {
                    return None;
                }
                CanonicalIndexInt::from_canonical_i64(parsed).map(Self::Number)
            }
            Self::UniqueSymbol(_) => None,
        }
    }
}

impl<I: PartialEq> PropertyKey<I> {
    /// Element-access collision: two keys address the same JS property.
    #[must_use]
    pub fn element_access_collides(&self, other: &Self) -> bool {
        self == other || self.element_access_equivalent().as_ref() == Some(other)
    }
}

impl<I> From<&str> for PropertyKey<I> {
    fn from(value: &str) -> Self {
        Self::String(Arc::from(value))
    }
}

impl<I> From<String> for PropertyKey<I> {
    fn from(value: String) -> Self {
        Self::String(Arc::from(value))
    }
}

impl<I> From<Arc<str>> for PropertyKey<I> {
    fn from(value: Arc<str>) -> Self {
        Self::String(value)
    }
}

/// Authored key carrier. A computed key retains the owning layer's typed child
/// instead of being omitted or coerced to display text.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    verter_no_typeexpr::NoTypeExpr,
    verter_no_storedspan::NoStoredSpan,
)]
pub enum AuthoredPropertyKey<C, I> {
    String(Arc<str>),
    Number(CanonicalIndexInt),
    UniqueSymbol(I),
    Computed(C),
}

impl<C, I> From<&str> for AuthoredPropertyKey<C, I> {
    fn from(value: &str) -> Self {
        Self::String(Arc::from(value))
    }
}

impl<C, I> From<String> for AuthoredPropertyKey<C, I> {
    fn from(value: String) -> Self {
        Self::String(Arc::from(value))
    }
}

impl<C, I> From<Arc<str>> for AuthoredPropertyKey<C, I> {
    fn from(value: Arc<str>) -> Self {
        Self::String(value)
    }
}

impl<C, I> AuthoredPropertyKey<C, I> {
    /// Construct the normalized ordinary string-key form.
    #[must_use]
    pub fn string(name: impl Into<Arc<str>>) -> Self {
        Self::String(name.into())
    }

    /// Lift a statically known key into the authored-key carrier.
    #[must_use]
    pub fn from_known(key: PropertyKey<I>) -> Self {
        match key {
            PropertyKey::String(value) => Self::String(value),
            PropertyKey::Number(value) => Self::Number(value),
            PropertyKey::UniqueSymbol(identity) => Self::UniqueSymbol(identity),
        }
    }

    /// Borrow the statically known key, retaining its exact identity class.
    #[must_use]
    pub fn as_known(&self) -> Option<PropertyKey<&I>> {
        match self {
            Self::String(value) => Some(PropertyKey::String(Arc::clone(value))),
            Self::Number(value) => Some(PropertyKey::Number(*value)),
            Self::UniqueSymbol(identity) => Some(PropertyKey::UniqueSymbol(identity)),
            Self::Computed(_) => None,
        }
    }

    /// Clone the exact statically known key identity.
    #[must_use]
    pub fn cloned_known(&self) -> Option<PropertyKey<I>>
    where
        I: Clone,
    {
        match self {
            Self::String(value) => Some(PropertyKey::String(Arc::clone(value))),
            Self::Number(value) => Some(PropertyKey::Number(*value)),
            Self::UniqueSymbol(identity) => Some(PropertyKey::UniqueSymbol(identity.clone())),
            Self::Computed(_) => None,
        }
    }

    /// Consume the authored key when it is statically known.
    pub fn into_known(self) -> Result<PropertyKey<I>, C> {
        match self {
            Self::String(value) => Ok(PropertyKey::String(value)),
            Self::Number(value) => Ok(PropertyKey::Number(value)),
            Self::UniqueSymbol(identity) => Ok(PropertyKey::UniqueSymbol(identity)),
            Self::Computed(child) => Err(child),
        }
    }

    /// Borrow the ordinary string spelling when statically known.
    #[must_use]
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            Self::Number(_) | Self::UniqueSymbol(_) | Self::Computed(_) => None,
        }
    }

    /// The PUBLISHED name for string-named publication surfaces: an
    /// ordinary string key verbatim, and a numeric literal key by its
    /// canonical ECMAScript spelling — `{ 1: x }` IS the property `"1"`
    /// (JS property identity coerces the numeric key), so a string-named
    /// surface carries it under that spelling. `None` for symbol /
    /// computed keys, which no string-named surface can carry.
    ///
    /// This is the ONE derivation every publication boundary reads — a
    /// site that re-derives a published name on its own can disagree with
    /// the rest of the boundary about which members the surface declares.
    #[must_use]
    pub fn published_name(&self) -> Option<Arc<str>> {
        match self {
            Self::String(value) => Some(Arc::clone(value)),
            Self::Number(value) => Some(Arc::from(value.get().to_string())),
            Self::UniqueSymbol(_) | Self::Computed(_) => None,
        }
    }

    /// Translate the computed-child and nominal-identity payloads without
    /// changing the authored key form.
    pub fn map<D, J>(
        self,
        computed: impl FnOnce(C) -> D,
        identity: impl FnOnce(I) -> J,
    ) -> AuthoredPropertyKey<D, J> {
        match self {
            Self::String(value) => AuthoredPropertyKey::String(value),
            Self::Number(value) => AuthoredPropertyKey::Number(value),
            Self::UniqueSymbol(value) => AuthoredPropertyKey::UniqueSymbol(identity(value)),
            Self::Computed(child) => AuthoredPropertyKey::Computed(computed(child)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CanonicalIndexInt;

    #[test]
    fn canonical_index_int_rejects_noncanonical_integer_spellings() {
        assert_eq!(
            CanonicalIndexInt::from_canonical_i64(42).map(CanonicalIndexInt::get),
            Some(42)
        );
        assert_eq!(CanonicalIndexInt::from_js_number(-0.0).unwrap().get(), 0);
        assert!(CanonicalIndexInt::from_js_number(1.5).is_none());
        assert!(CanonicalIndexInt::from_canonical_i64(4_611_686_018_427_387_904).is_none());
    }
}

#[cfg(test)]
mod authored_property_key_contract_tests {
    use super::{AuthoredPropertyKey, CanonicalIndexInt, PropertyKey};
    use crate::{FunctionExpr, MethodSignature, ObjectProperty, TypeExpr, ValueDeclIdentityPart};
    use std::sync::Arc;

    fn symbol(canonical_id: &str) -> ValueDeclIdentityPart {
        ValueDeclIdentityPart {
            canonical_id: Arc::from(canonical_id),
            owner: crate::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("token"),
            member_path: Arc::from([]),
        }
    }

    #[test]
    fn authored_property_keys_preserve_equivalence_numeric_and_nominal_identity() {
        assert_eq!(
            PropertyKey::<ValueDeclIdentityPart>::identifier("answer"),
            PropertyKey::<ValueDeclIdentityPart>::string_literal("answer"),
        );
        assert_eq!(
            CanonicalIndexInt::from_js_number(42.0)
                .map(PropertyKey::<ValueDeclIdentityPart>::Number),
            Some(PropertyKey::Number(
                CanonicalIndexInt::from_canonical_i64(42).unwrap()
            )),
        );
        assert!(CanonicalIndexInt::from_js_number(1.5).is_none());

        let first = PropertyKey::UniqueSymbol(symbol("/first.ts"));
        let second = PropertyKey::UniqueSymbol(symbol("/second.ts"));
        assert_ne!(
            first, second,
            "same display spelling is not nominal identity"
        );

        let computed: AuthoredPropertyKey<TypeExpr, ValueDeclIdentityPart> =
            AuthoredPropertyKey::Computed(TypeExpr::named("K"));
        assert!(matches!(
            computed,
            AuthoredPropertyKey::Computed(TypeExpr::Ref { .. })
        ));
    }

    #[test]
    fn object_members_retain_typed_known_and_computed_keys() {
        let numeric =
            AuthoredPropertyKey::Number(CanonicalIndexInt::from_canonical_i64(42).unwrap());
        let property = ObjectProperty::synthetic_public_key(
            numeric.clone(),
            TypeExpr::Primitive(crate::PrimitiveName::String),
            false,
            false,
        );
        assert_eq!(property.key, numeric);

        let computed = AuthoredPropertyKey::Computed(TypeExpr::named("K"));
        let method = MethodSignature::synthetic_public_key(
            computed.clone(),
            FunctionExpr::synthetic(Vec::new(), None, Vec::new()),
            false,
        );
        assert_eq!(method.key, computed);
    }
}
