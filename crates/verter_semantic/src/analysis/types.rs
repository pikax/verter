use verter_session_query::analysis::types::deserialize_analysis_flags;
use verter_session_query::analysis::types::is_false;
use verter_session_query::analysis::types::serialize_analysis_flags;
use verter_session_query::analysis::types::{
    AnalysisFlags, AnalyzedBinding, AnalyzedExportedFunction, AnalyzedImport, AnalyzedMacro,
    AnalyzedModuleReference, AnalyzedOptionsApi, CssVarManipulation, DomQueryKind,
    LocalDeclarationEntry, MacroTypeDep, NestedMacroCall, ScriptBindingOccurrence,
    ScriptTypeEnhancements, StoreDefinition, StoreUsage, VueApiCallSite,
};
use verter_span::Span;

// ---------------------------------------------------------------------------
// Stable declaration identifiers
// ---------------------------------------------------------------------------

/// Comprehensive script analysis captured during SFC parsing.
/// Powers dependency tracking, type-aware linting, and codegen optimization.
///
/// This is an immutable post-parse artifact: once produced it is never
/// mutated, and the host shares it across readers via
/// `Arc<ScriptAnalysisSnapshot>` rather than deep-copying ~18 owned vectors
/// per read. `Clone` (derived) performs a genuine field-by-field deep copy and
/// is reserved for the rare caller that needs an owned copy; the hot read path
/// `Arc::clone`s the shared handle instead, which is a refcount bump.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptAnalysisSnapshot {
    /// All import declarations found in the script block(s).
    pub imports: Vec<AnalyzedImport>,

    /// All module reference sites found in the script block(s).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub module_references: Vec<AnalyzedModuleReference>,

    /// Top-level bindings (variables, functions, classes).
    pub bindings: Vec<AnalyzedBinding>,

    /// Vue macro calls detected in script setup.
    pub macros: Vec<AnalyzedMacro>,

    /// Which imported types are used by which macros (derived from macros + imports).
    pub macro_type_deps: Vec<MacroTypeDep>,

    /// Bitwise flags for quick queries (serialized as raw u32 bits).
    #[serde(
        serialize_with = "serialize_analysis_flags",
        deserialize_with = "deserialize_analysis_flags"
    )]
    pub flags: AnalysisFlags,

    /// Exported functions with return type analysis (when FUNC_RETURNS scope is active).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exported_functions: Vec<AnalyzedExportedFunction>,

    /// Vue API function call sites (lifecycle hooks, watchers, provide/inject, etc.).
    /// Tracks calls whose return values are typically discarded (e.g., `onMounted(cb)`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vue_api_calls: Vec<VueApiCallSite>,

    /// DOM query call sites (querySelector, getElementById, etc.).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dom_query_calls: Vec<DomQueryCallSite>,

    /// CSS variable manipulations via DOM style APIs (setProperty, getPropertyValue, removeProperty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub css_var_manipulations: Vec<CssVarManipulation>,

    /// Script-side binding usage occurrences with exact spans.
    /// Populated when `AnalysisScope::SCRIPT_USAGES` is active.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub script_binding_occurrences: Vec<ScriptBindingOccurrence>,

    /// Script-side usage facts for macro-declared members (unused-declaration
    /// diagnostics). `None` for files without Vue macros.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macro_usage: Option<crate::analysis::macro_usage::MacroUsageFacts>,

    /// SFC-absolute byte offset of the first top-level `await` expression (if any).
    /// Used by lint rules to detect lifecycle hooks/watchers called after await.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_await_offset: Option<u32>,

    /// TODO(type-provider): Enhanced type info populated by TSGO when connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_enhancements: Option<ScriptTypeEnhancements>,

    /// Options API analysis extracted from `export default { ... }` or
    /// `export default defineComponent({ ... })`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options_api: Option<AnalyzedOptionsApi>,

    /// Vue compiler macro calls found inside nested scopes (functions, conditionals, loops).
    /// These macros must be at root level in `<script setup>` — nested calls are invalid.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nested_macro_calls: Vec<NestedMacroCall>,

    /// Store usage sites (Pinia, Vuex, convention-based composable stores).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store_usages: Vec<StoreUsage>,

    /// Store definitions (`defineStore`, `createStore`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store_definitions: Vec<StoreDefinition>,

    /// Whether the script block uses TypeScript (`lang="ts"`).
    ///
    /// Used by lint rules that only apply to TypeScript SFCs (e.g., `define-props-declaration`,
    /// `define-emits-declaration`, `define-model-type-required`, `require-typed-ref`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_typescript: bool,

    /// Top-level declaration entries: types, values, and class/enum (both).
    ///
    /// Provides a unified view of all declarations in the file for the resolver.
    /// Each entry has a content hash for change detection and a span for position.
    /// Populated during `build_script_analysis`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declaration_entries: Vec<LocalDeclarationEntry>,

    /// Root identifiers referenced by `<style>` `v-bind()` expressions
    /// (`"color"` from `v-bind(color)`, `"theme"` from `v-bind(theme.color)`),
    /// recorded by [`Self::mark_bindings_used_in_style`]. CSS `v-bind()`
    /// resolves through the component's render context, which includes PROPS
    /// by bare name — so the unused-declaration population consumes this set
    /// for prop-member liveness, not just script-binding `used_in_style`.
    /// Sorted + deduplicated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub style_vbind_roots: Vec<String>,
}

impl ScriptAnalysisSnapshot {
    /// Mark bindings that are referenced by CSS `v-bind()` expressions in style blocks.
    ///
    /// For each `v-bind(expr)` found in style analysis, extracts the root identifier
    /// (e.g., `"color"` from `v-bind(color)`, `"theme"` from `v-bind(theme.color)`)
    /// and sets `used_in_style = true` on the matching binding. The full root
    /// set is also retained on [`Self::style_vbind_roots`] — style `v-bind()`
    /// can reference PROPS by bare name (no script binding exists for those),
    /// and prop-member liveness needs the raw set.
    pub fn mark_bindings_used_in_style(
        &mut self,
        style_analyses: &[crate::analysis::style::StyleBlockAnalysis],
    ) {
        // The SOUND per-expression root facts recorded by the producer
        // (`AnalyzedVBind.expr_roots`, OXC-derived) are the sole usage
        // authority — never a text split of the expression.
        let mut referenced: rustc_hash::FxHashSet<&str> = rustc_hash::FxHashSet::default();
        let mut any_v_bind = false;
        let mut all_complete = true;
        // An external `<style src="...">` block's content is DEFERRED (B-23):
        // its usage facts are unknowable, which is the same epistemic state as
        // an unparseable v-bind expression — fail OPEN below.
        let external_deferred = style_analyses
            .iter()
            .any(|style| !style.content_is_available());
        for vb in style_analyses.iter().flat_map(|s| &s.v_binds) {
            any_v_bind = true;
            if !vb.roots_complete {
                all_complete = false;
            }
            referenced.extend(
                vb.expr_roots
                    .iter()
                    .map(String::as_str)
                    .filter(|n| !n.is_empty()),
            );
        }

        if !any_v_bind && !external_deferred {
            return;
        }

        let mut roots: Vec<String> = referenced.iter().map(|name| name.to_string()).collect();
        roots.sort_unstable();
        self.style_vbind_roots = roots;

        if !all_complete || external_deferred {
            // An unparseable v-bind expression, or a deferred external style
            // sheet whose `v-bind()` usage cannot be seen: fail OPEN — every
            // binding is treated as style-used so no false unused diagnostic
            // can fire.
            for binding in &mut self.bindings {
                binding.used_in_style = true;
            }
            return;
        }

        if referenced.is_empty() {
            return;
        }

        for binding in &mut self.bindings {
            if referenced.contains(binding.name.as_str()) {
                binding.used_in_style = true;
            }
        }
    }
}

/// A DOM query call site found in the script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomQueryCallSite {
    /// Which DOM query API was called.
    pub kind: DomQueryKind,
    /// The raw string argument (selector text or ID).
    pub selector_text: String,
    /// Parsed selector structure (reuses the CSS selector parser).
    pub parsed: Option<crate::analysis::style::StructuredSelector>,
    /// SFC-absolute byte span of the call expression.
    pub span: Span,
    /// SFC-absolute byte span of just the string argument.
    pub arg_span: Span,
}

impl serde::Serialize for DomQueryCallSite {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 6 + usize::from(self.parsed.is_some());
        let mut s = serializer.serialize_struct("DomQueryCallSite", count)?;
        s.serialize_field("kind", &self.kind)?;
        s.serialize_field("selectorText", &self.selector_text)?;
        if self.parsed.is_some() {
            s.serialize_field("parsed", &self.parsed)?;
        }
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.serialize_field("argSpanStart", &self.arg_span.start)?;
        s.serialize_field("argSpanEnd", &self.arg_span.end)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for DomQueryCallSite {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            kind: DomQueryKind,
            selector_text: String,
            #[serde(default)]
            parsed: Option<crate::analysis::style::StructuredSelector>,
            span_start: u32,
            span_end: u32,
            arg_span_start: u32,
            arg_span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            kind: w.kind,
            selector_text: w.selector_text,
            parsed: w.parsed,
            span: Span::new(w.span_start, w.span_end),
            arg_span: Span::new(w.arg_span_start, w.arg_span_end),
        })
    }
}

// ── Options API Analysis Types ──

// ── Non-SFC Deep Analysis ──

// ── Type Enhancement Placeholders ──

// =============================================================================
// Store / State Management
// =============================================================================

#[cfg(test)]
mod analyzed_macro_serde_tests {
    //! Plan Step 1 / D1.2 + sub-task 1.4 — serialization integrity for
    //! `AnalyzedMacro::parsed_type_argument`. The struct uses manual
    //! `Serialize` / `Deserialize` impls (types.rs:1276 / 1322), so a
    //! string-typo in the camelCase field name would silently drop the
    //! field on the wire. These tests catch that.
    //!
    //! FAIL-FIRST contract: writing the field literal `parsedTypeArgument`
    //! into the manual serializer is required for `serializes_field_name_exactly`
    //! to pass. The back-compat test verifies old-shape payloads (no
    //! `parsedTypeArgument` key) deserialize with `parsed_type_argument: None`
    //! — discriminating because if the deserializer's `Wire` struct were
    //! to drop `#[serde(default)]` on the field, the test would fail with
    //! "missing field" error.
    use std::sync::Arc;
    use verter_session_query::analysis::types::{
        AnalyzedMacro, AnalyzedMacroKind, MacroAnchor, MacroAnchorUnsupported, MacroEditAnchors,
        MemberListAnchor,
    };
    use verter_span::Span;
    use verter_type_expr::locators::{
        AuthoredAnchor, LocatorSymbolSpace, MacroPayloadLocator, MacroPayloadPosition,
    };
    use verter_type_expr::TopLevelOwnerId;

    fn sample_payload_locator(symbol: &str) -> MacroPayloadLocator {
        MacroPayloadLocator {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from(""),
                owner: TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from(symbol),
                space: LocatorSymbolSpace::Value,
            },
            macro_index: 0,
            payload: MacroPayloadPosition::TypeArgument,
        }
    }

    fn empty_macro() -> AnalyzedMacro {
        AnalyzedMacro {
            edit_anchors: Default::default(),
            kind: AnalyzedMacroKind::DefineProps,
            owner: TopLevelOwnerId::ordinary_file(),
            is_type_based: true,
            type_references: Vec::new(),
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 0),
        }
    }

    #[test]
    fn serializes_parsed_type_argument_field_name_exactly() {
        let mut m = empty_macro();
        m.parsed_type_argument = Some(sample_payload_locator("Sentinel"));
        let json = serde_json::to_string(&m).unwrap();
        // Field name appears EXACTLY (catches typo in manual serialize_field).
        assert!(
            json.contains("\"parsedTypeArgument\":"),
            "manual Serialize did not emit the parsedTypeArgument field; \
             check serialize_field call in AnalyzedMacro::serialize. JSON: {json}"
        );
        // Sentinel value also appears (catches the value being silently dropped).
        assert!(
            json.contains("\"Sentinel\""),
            "parsed_type_argument value not in serialized output; JSON: {json}"
        );

        // Round-trip integrity.
        let roundtripped: AnalyzedMacro = serde_json::from_str(&json).unwrap();
        assert_eq!(m.parsed_type_argument, roundtripped.parsed_type_argument);
    }

    #[test]
    fn serializes_none_omits_field() {
        let m = empty_macro();
        assert!(m.parsed_type_argument.is_none());
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            !json.contains("parsedTypeArgument"),
            "None values should be omitted from output; JSON: {json}"
        );
    }

    #[test]
    fn deserializes_old_payload_without_parsed_type_argument_field() {
        // Construct an old-shape payload (no parsedTypeArgument key).
        // Mirrors what existing serialized AnalyzedMacro JSON on disk
        // would have looked like before D1.2 added the field.
        let old_json = r#"{
            "kind": "DefineProps",
            "isTypeBased": true,
            "typeReferences": [],
            "bindingName": null,
            "hasInheritAttrsFalse": false,
            "spanStart": 0,
            "spanEnd": 0
        }"#;
        let parsed: AnalyzedMacro = serde_json::from_str(old_json).unwrap();
        assert_eq!(
            parsed.parsed_type_argument, None,
            "old-shape payload (no parsedTypeArgument key) must \
             deserialize with parsed_type_argument: None"
        );
    }

    // ── AnalyzedMacro edit anchors: wire back-compat ──

    /// An anchor-less macro adds no key (P4: no hot-payload growth), and a
    /// payload with no `editAnchors` key deserializes to the typed unsupported
    /// default rather than a fabricated position.
    #[test]
    fn analyzed_macro_serde_roundtrip_omits_absent_anchor_keys() {
        let m = empty_macro();
        assert!(m.edit_anchors.is_absent(), "fixture carries no anchors");
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            !json.contains("editAnchors"),
            "an anchor-less macro must not grow the payload; JSON: {json}"
        );

        let old_json = r#"{
            "kind": "DefineSlots",
            "isTypeBased": true,
            "typeReferences": [],
            "bindingName": null,
            "hasInheritAttrsFalse": false,
            "spanStart": 0,
            "spanEnd": 0
        }"#;
        let parsed: AnalyzedMacro = serde_json::from_str(old_json).unwrap();
        assert_eq!(
            parsed.edit_anchors,
            MacroEditAnchors::default(),
            "an absent editAnchors key must deserialize to the typed unsupported default"
        );
        assert_eq!(
            parsed.edit_anchors.type_literal.unsupported_reason(),
            Some(MacroAnchorUnsupported::NoTypeArgument),
            "the default is an honest typed absence, not an available anchor"
        );
        assert!(
            parsed.edit_anchors.type_literal.available().is_none(),
            "an absent key must never yield an editable position"
        );
    }

    /// A populated anchor round-trips its offset, its emptiness flag, and each
    /// distinct unsupported reason without collapsing them.
    #[test]
    fn analyzed_macro_serde_roundtrip_preserves_anchor_values() {
        let mut m = empty_macro();
        m.edit_anchors = MacroEditAnchors {
            type_literal: MacroAnchor::Available(MemberListAnchor::new(41, false)),
            runtime_array: MacroAnchor::Unsupported(MacroAnchorUnsupported::NoMemberList),
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            json.contains("\"editAnchors\":"),
            "a populated anchor set must be emitted; JSON: {json}"
        );

        let back: AnalyzedMacro = serde_json::from_str(&json).unwrap();
        assert_eq!(m.edit_anchors, back.edit_anchors);
        let anchor = back
            .edit_anchors
            .type_literal
            .available()
            .expect("round-trip keeps the available arm");
        assert_eq!(anchor.insert_offset(), 41);
        assert!(!anchor.is_empty());
        // Negative: the sibling reason survives distinctly, not collapsed into
        // the `NoTypeArgument` default.
        assert_eq!(
            back.edit_anchors.runtime_array.unsupported_reason(),
            Some(MacroAnchorUnsupported::NoMemberList)
        );
        assert_ne!(
            back.edit_anchors.runtime_array.unsupported_reason(),
            Some(MacroAnchorUnsupported::NoTypeArgument)
        );
    }

    /// Each producer reason is a distinct wire value — reasons never collapse.
    #[test]
    fn macro_anchor_unsupported_reasons_are_distinct_on_the_wire() {
        let reasons = [
            MacroAnchorUnsupported::NoTypeArgument,
            MacroAnchorUnsupported::NotTypeBased,
            MacroAnchorUnsupported::NamedTypeArgument,
            MacroAnchorUnsupported::IntersectionTypeArgument,
            MacroAnchorUnsupported::NoMemberList,
        ];
        let encoded: Vec<String> = reasons
            .iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect();
        let unique: std::collections::BTreeSet<&String> = encoded.iter().collect();
        assert_eq!(
            unique.len(),
            reasons.len(),
            "every reason must encode distinctly; got {encoded:?}"
        );
        for (reason, json) in reasons.iter().zip(encoded.iter()) {
            let back: MacroAnchorUnsupported = serde_json::from_str(json).unwrap();
            assert_eq!(&back, reason);
        }
    }

    #[test]
    fn roundtrip_preserves_payload_locator_structure() {
        let mut m = empty_macro();
        // Construct a non-trivial locator (a field payload position) so the
        // round-trip test exercises the non-empty serialization path.
        m.parsed_type_argument = Some(MacroPayloadLocator {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from(""),
                owner: TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from("default"),
                space: LocatorSymbolSpace::Value,
            },
            macro_index: 2,
            payload: MacroPayloadPosition::Field { field_index: 5 },
        });
        let json = serde_json::to_string(&m).unwrap();
        let back: AnalyzedMacro = serde_json::from_str(&json).unwrap();
        assert_eq!(m.parsed_type_argument, back.parsed_type_argument);
    }
}
