//! The path from a macro's parent type to one of its fields.

/// Path segment for [`FieldExpansionContext::output_path`] — a path from
/// the parent macro shell (e.g. `Props<T>`) to the specific field the
/// closure is being invoked for. The session-side closure converts this
/// into a `verter_session::semantic_query::PathSegment` slice when
/// constructing the dispatch projection query (plan Step 1 / D1.1).
///
/// `Member` is the only variant required for Step 1 — `defineProps`,
/// `defineEmits`, and `defineSlots` all expose fields at named members
/// of the macro's parent type. Future variants (`Index`, `KeyOf`) are
/// deferred until a consumer needs them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PathSegment {
    /// Named-member hop, e.g. `[Member("items")]` for the `items` prop
    /// field of `defineProps<Props>()`.
    Member(std::sync::Arc<str>),
}
