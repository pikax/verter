//! Owned records of the compiler-macro calls a script makes.

use verter_span::Span;

/// A literal emit call site: `emit("save", …)`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroUsageCall {
    /// The literal event name.
    pub name: String,
    /// SFC-absolute byte span of the call expression.
    pub span: Span,
}

/// Script-side usage facts for macro-declared members (see module docs).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroUsageFacts {
    /// Literal event names called on the `defineEmits` binding, with spans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emit_literal_calls: Vec<MacroUsageCall>,
    /// The emit binding escaped literal-call analysis (aliased, passed as a
    /// value, called with a dynamic event name, …). Suppresses ALL
    /// unused-event diagnostics for the component.
    pub emit_escapes: bool,
    /// Literal member names read off the `defineProps` binding (`props.x`,
    /// `props["x"]`, `toRef(props, "x")`, immediately-destructured
    /// `toRefs(props)` keys).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props_member_reads: Vec<String>,
    /// The props binding escaped member-read analysis (spread, call argument,
    /// alias, computed access, return, …). Suppresses ALL unused-prop
    /// diagnostics for the component.
    pub props_escapes: bool,
    /// `defineProps`/`withDefaults` was DESTRUCTURED — destructured member
    /// liveness is provider-owned (TS6133); the native unused-prop diagnostic
    /// must not double-report.
    pub props_destructured: bool,
}
