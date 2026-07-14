//! Generated static-intrinsic catalog substrate — the NoTypeExpr replacement for
//! the raw-`TypeExpr` HTML intrinsic member shapes.
//!
//! An intrinsic member (an HTML attribute or event listener) is a member of a
//! GENERATED static catalog. Rather than allocating an `Unknown { raw }` string
//! (listener text, non-primitive attr text) into a semantic fact, a member fact
//! stores a content-free [`StaticIntrinsicTypeId`] — an interned id into the
//! generated catalog. The generated lookup table ([`StaticIntrinsicTable`]) maps
//! that id to its static shape for lowering / display, so the listener function
//! text is preserved in the TABLE (lookup infrastructure), never in the fact.
//!
//! This module owns the id newtype, the lower-neutral member fact
//! ([`IntrinsicMemberFact`] + [`IntrinsicMemberKind`]) — all witnessed fact
//! carriers — and the interner table interface. The GENERATED catalog data
//! (the id → shape population + the concrete shape schema) is owned by the HTML
//! intrinsics catalog that consumes this substrate: the table is generic over
//! the shape so that catalog picks its own shape (which may hold `&'static str`
//! display text — legal, because the shape lives in the table, not in a fact).

use rustc_hash::FxHashMap;
use std::hash::Hash;
use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

/// Kind of intrinsic member — the lower-neutral role classification carried in an
/// [`IntrinsicMemberFact`].
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum IntrinsicMemberKind {
    /// An HTML attribute (e.g. `id`, `disabled`, `placeholder`).
    Attr,
    /// An event listener (e.g. `click`, `focus`) — the name is the event name,
    /// not the `onXxx` form.
    Listener,
}

/// An interned id into the generated static-intrinsic catalog.
///
/// This is a content-free ordinal into the catalog's [`StaticIntrinsicTable`] —
/// NOT a stored `Primitive`/`Unknown` shape. The shape lives in the table keyed
/// by this id, so an [`IntrinsicMemberFact`] stores only this id: a listener's
/// function text (or a non-primitive attr type) is preserved in the table, never
/// allocated as an `Unknown { raw }` string into a semantic fact.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct StaticIntrinsicTypeId(u32);

impl StaticIntrinsicTypeId {
    /// Reconstruct an id from its raw ordinal (e.g. deserializing a fact, or the
    /// table minting the next id). Fabricating an out-of-range ordinal is not a
    /// safety hazard — [`StaticIntrinsicTable::shape`] simply returns `None`.
    #[must_use]
    pub const fn from_u32(raw: u32) -> Self {
        Self(raw)
    }

    /// The raw catalog ordinal.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

/// A single intrinsic member fact (an attribute or listener) — the NoTypeExpr
/// replacement for the raw-`TypeExpr` intrinsic member. Stores member identity
/// (`name`), its role (`kind`), and the content-free catalog `type_id`; the
/// member's type SHAPE is recovered from the catalog table by `type_id`, never
/// stored as a `TypeExpr` here.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct IntrinsicMemberFact {
    /// The member name (event name for a listener; attribute name for an attr).
    pub name: String,
    /// The member role.
    pub kind: IntrinsicMemberKind,
    /// The interned catalog id of the member's type shape.
    pub type_id: StaticIntrinsicTypeId,
}

/// The generated static-intrinsic catalog table — the id ↔ shape interner that
/// mints [`StaticIntrinsicTypeId`]s and maps them back to their static shape for
/// lowering / display.
///
/// This is lookup INFRASTRUCTURE, not a fact carrier: it is a mutable interner
/// (a `Vec` + dedup index), so it is deliberately NOT witnessed
/// `NoTypeExpr`/`NoStoredSpan` — only the ids it mints are fact carriers. It is
/// generic over the shape `S` so the generated catalog owns the concrete shape
/// schema (which may hold `&'static str` display text — legal, because the shape
/// is table-resident, not fact-resident).
///
/// Interning is stable and deduplicating: the same shape always interns to the
/// same id, and [`shape`](Self::shape) round-trips an id back to its shape.
#[derive(Debug, Clone)]
pub struct StaticIntrinsicTable<S> {
    /// id (index) → shape.
    shapes: Vec<S>,
    /// shape → id, for dedup (an equal shape interns to the same id).
    index: FxHashMap<S, StaticIntrinsicTypeId>,
}

impl<S> StaticIntrinsicTable<S>
where
    S: Eq + Hash + Clone,
{
    /// A new, empty catalog table.
    #[must_use]
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            index: FxHashMap::default(),
        }
    }

    /// Intern a shape, returning its stable catalog id. An equal shape already in
    /// the table returns its existing id (dedup); a new shape is appended and
    /// gets the next sequential ordinal.
    pub fn intern(&mut self, shape: S) -> StaticIntrinsicTypeId {
        if let Some(&id) = self.index.get(&shape) {
            return id;
        }
        let id = StaticIntrinsicTypeId::from_u32(
            u32::try_from(self.shapes.len()).expect("static intrinsic catalog id space exhausted"),
        );
        self.shapes.push(shape.clone());
        self.index.insert(shape, id);
        id
    }

    /// The shape for a catalog id, or `None` if the id is out of range (e.g. a
    /// fabricated ordinal). Round-trips [`intern`](Self::intern).
    #[must_use]
    pub fn shape(&self, id: StaticIntrinsicTypeId) -> Option<&S> {
        self.shapes.get(id.as_u32() as usize)
    }

    /// The already-interned id for an EQUAL shape, or `None` when the shape was
    /// never interned. The read-only counterpart of [`intern`](Self::intern) for
    /// consumers of a fully-built catalog (no insertion, no growth).
    #[must_use]
    pub fn id_for(&self, shape: &S) -> Option<StaticIntrinsicTypeId> {
        self.index.get(shape).copied()
    }

    /// The number of distinct interned shapes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    /// Whether the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }
}

impl<S> Default for StaticIntrinsicTable<S>
where
    S: Eq + Hash + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_intrinsic_type_id_round_trips_its_ordinal() {
        // The id is a transparent ordinal newtype: `from_u32`/`as_u32` round-trip
        // and the id participates in equality by that ordinal.
        let id = StaticIntrinsicTypeId::from_u32(7);
        assert_eq!(id.as_u32(), 7);
        assert_eq!(id, StaticIntrinsicTypeId::from_u32(7));
        assert_ne!(id, StaticIntrinsicTypeId::from_u32(8));
    }

    #[test]
    fn interning_is_stable_and_deduplicating_and_round_trips() {
        // A representative catalog shape: primitives lower directly; other shapes
        // (listener text, non-primitive attr types) are preserved as display
        // text in the table — NEVER as an `Unknown { raw }` in a fact.
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        enum Shape {
            Primitive(&'static str),
            Display(&'static str),
        }

        let mut table: StaticIntrinsicTable<Shape> = StaticIntrinsicTable::new();
        assert!(table.is_empty());

        let string_id = table.intern(Shape::Primitive("string"));
        let number_id = table.intern(Shape::Primitive("number"));
        let listener_id = table.intern(Shape::Display("(payload: Event) => void"));

        // Distinct shapes intern to distinct ids.
        assert_ne!(string_id, number_id);
        assert_ne!(string_id, listener_id);
        assert_ne!(number_id, listener_id);
        assert_eq!(table.len(), 3);

        // Stable: re-interning an EQUAL shape returns the SAME id (dedup by value
        // through the shape's `Eq`/`Hash`, no growth).
        assert_eq!(table.intern(Shape::Primitive("string")), string_id);
        assert_eq!(
            table.intern(Shape::Display("(payload: Event) => void")),
            listener_id
        );
        assert_eq!(table.len(), 3, "dedup must not grow the table");

        // Round-trip: id → shape recovers the interned shape.
        assert_eq!(table.shape(string_id), Some(&Shape::Primitive("string")));
        assert_eq!(
            table.shape(listener_id),
            Some(&Shape::Display("(payload: Event) => void"))
        );
        // An out-of-range (fabricated) id has no shape — no panic, honest `None`.
        assert_eq!(table.shape(StaticIntrinsicTypeId::from_u32(999)), None);
    }

    #[test]
    fn intrinsic_member_fact_discriminates_on_every_axis() {
        let base = IntrinsicMemberFact {
            name: "click".to_string(),
            kind: IntrinsicMemberKind::Listener,
            type_id: StaticIntrinsicTypeId::from_u32(0),
        };
        // Each of name / kind / type_id must independently discriminate identity.
        assert_ne!(
            base,
            IntrinsicMemberFact {
                name: "focus".to_string(),
                ..base.clone()
            },
            "name must discriminate"
        );
        assert_ne!(
            base,
            IntrinsicMemberFact {
                kind: IntrinsicMemberKind::Attr,
                ..base.clone()
            },
            "kind must discriminate"
        );
        assert_ne!(
            base,
            IntrinsicMemberFact {
                type_id: StaticIntrinsicTypeId::from_u32(1),
                ..base.clone()
            },
            "type_id must discriminate"
        );
        assert_eq!(base, base.clone());
    }
}
