//! Lint diagnostic types.
//!
//! Detection only — diagnostics carry span + severity + tags.
//! Fixes live in `verter_actions`.

/// Severity level for a lint diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Error: must fix.
    Error,
    /// Warning: should fix.
    Warning,
    /// Informational: style suggestion.
    Info,
    /// Hint: faded out, lowest priority.
    Hint,
}

/// Optional tags that modify how a diagnostic is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticTag {
    /// Marks the diagnostic span as unnecessary (e.g., unused CSS selector).
    Unnecessary,
    /// Marks the diagnostic span as deprecated.
    Deprecated,
}

/// Describes what the diagnostic span refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSpanKind {
    /// Opening tag: `<div ...>` (span_start..tag_span_end)
    ElementOpenTag,
    /// Full element including children: `<div>...</div>`
    FullElement,
    /// Attribute name+value: `class="foo"`, `:class="expr"`, `autofocus`
    Attribute,
    /// A directive: `v-for="..."`, `@click`, `v-html`
    Directive,
    /// CSS selector in a style block
    CssSelector,
    /// CSS class name in a template class attribute
    CssClassName,
    /// Template interpolation: `{{ expr }}`
    Interpolation,
    /// Condition expression in v-if/v-else-if chain
    ConditionExpression,
    /// Script-level call site (lifecycle, Vue API, composable)
    ScriptCallSite,
    /// Cross-file reference (provide/inject/composable chain)
    CrossFileEntry,
    /// Prop definition in defineProps
    PropDefinition,
    /// File-level diagnostic (no specific span)
    FileLevel,
    /// Element inner content area: text/interpolation between tags
    ElementContent,
}

/// Whether the diagnostic has full information or is based on incomplete analysis.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum Certainty {
    /// Rule has full information (single-file structural rules).
    #[default]
    Definite,
    /// Rule lacks some context (cross-file, heuristic, partial project scan).
    Partial,
}

/// A source snippet providing evidence for why a diagnostic was emitted.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceSnippet {
    /// 1-3 lines of source context around the diagnostic span.
    pub context: String,
    /// Byte offset within `context` where the highlighted region starts.
    pub highlight_start: u32,
    /// Byte offset within `context` where the highlighted region ends.
    pub highlight_end: u32,
}

/// A file related to a diagnostic finding (for cross-file diagnostics).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedFile {
    /// Path to the related file.
    pub path: String,
    /// Why this file is related to the diagnostic.
    pub reason: String,
}

/// A single lint diagnostic emitted by a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintDiagnostic {
    /// Rule name (e.g., `"require-v-for-key"`).
    pub rule: String,
    /// Rule category for grouping.
    pub category: String,
    /// Severity level.
    pub severity: Severity,
    /// Human-readable message.
    pub message: String,
    /// SFC-absolute byte offset span.
    pub span: verter_span::Span,
    /// Optional tags that modify rendering (e.g., `Unnecessary`, `Deprecated`).
    pub tags: Vec<DiagnosticTag>,
    /// What the diagnostic span refers to.
    pub span_kind: DiagnosticSpanKind,
    /// Whether the rule had complete information for this finding.
    pub certainty: Certainty,
    /// Source evidence snippets (populated on demand, usually empty).
    pub evidence: Vec<EvidenceSnippet>,
    /// Related files for cross-file diagnostics.
    pub related_files: Vec<RelatedFile>,
}

// Manual serde impls preserve the existing flat JSON field names
// (`"spanStart"` / `"spanEnd"`) for wire compatibility.

impl serde::Serialize for LintDiagnostic {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        // serde-wasm-bindgen v0.6 turns serialize_map into a JS Map (not a plain object),
        // so property access like `d.message` returns undefined. serialize_struct produces
        // a plain JS object, fixing both Lint and Diagnostics panels in the playground.
        let has_tags = !self.tags.is_empty();
        let has_evidence = !self.evidence.is_empty();
        let has_related = !self.related_files.is_empty();
        let len = 8
            + if has_tags { 1 } else { 0 }
            + if has_evidence { 1 } else { 0 }
            + if has_related { 1 } else { 0 };
        let mut state = serializer.serialize_struct("LintDiagnostic", len)?;
        state.serialize_field("rule", &self.rule)?;
        state.serialize_field("category", &self.category)?;
        state.serialize_field("severity", &self.severity)?;
        state.serialize_field("message", &self.message)?;
        state.serialize_field("spanStart", &self.span.start)?;
        state.serialize_field("spanEnd", &self.span.end)?;
        if has_tags {
            state.serialize_field("tags", &self.tags)?;
        }
        state.serialize_field("spanKind", &self.span_kind)?;
        state.serialize_field("certainty", &self.certainty)?;
        if has_evidence {
            state.serialize_field("evidence", &self.evidence)?;
        }
        if has_related {
            state.serialize_field("relatedFiles", &self.related_files)?;
        }
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for LintDiagnostic {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            rule: String,
            category: String,
            severity: Severity,
            message: String,
            span_start: u32,
            span_end: u32,
            #[serde(default)]
            tags: Vec<DiagnosticTag>,
            span_kind: DiagnosticSpanKind,
            #[serde(default)]
            certainty: Certainty,
            #[serde(default)]
            evidence: Vec<EvidenceSnippet>,
            #[serde(default)]
            related_files: Vec<RelatedFile>,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(LintDiagnostic {
            rule: w.rule,
            category: w.category,
            severity: w.severity,
            message: w.message,
            span: verter_span::Span::new(w.span_start, w.span_end),
            tags: w.tags,
            span_kind: w.span_kind,
            certainty: w.certainty,
            evidence: w.evidence,
            related_files: w.related_files,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_serde_roundtrip() {
        let diag = LintDiagnostic {
            rule: "require-v-for-key".to_string(),
            category: "vue-essential".to_string(),
            severity: Severity::Error,
            message: "Elements in iteration expect to have 'v-bind:key' directives.".to_string(),
            span: verter_span::Span::new(10, 40),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Directive,
            certainty: Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        };

        let json = serde_json::to_string(&diag).expect("serialize");
        let roundtrip: LintDiagnostic = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(diag, roundtrip);
        // Flat keys preserved
        assert!(json.contains("spanStart"), "should have spanStart key");
        assert!(json.contains("spanEnd"), "should have spanEnd key");
        // Empty tags should be omitted from JSON
        assert!(
            !json.contains("tags"),
            "empty tags should be skipped in JSON"
        );
    }

    #[test]
    fn diagnostic_with_tags_serde_roundtrip() {
        let diag = LintDiagnostic {
            rule: "unused-css-selector".to_string(),
            category: "css".to_string(),
            severity: Severity::Hint,
            message: "Selector '.foo' does not match any template elements".to_string(),
            span: verter_span::Span::new(5, 20),
            tags: vec![DiagnosticTag::Unnecessary],
            span_kind: DiagnosticSpanKind::CssSelector,
            certainty: Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        };

        let json = serde_json::to_string(&diag).expect("serialize");
        let roundtrip: LintDiagnostic = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(diag, roundtrip);
        assert!(
            json.contains("tags"),
            "non-empty tags should appear in JSON"
        );
        assert!(
            json.contains("unnecessary"),
            "tag should serialize as camelCase"
        );
    }

    /// Verify that serialize_struct produces a JSON object with direct field access.
    /// This is critical for serde-wasm-bindgen: serialize_map produces a JS Map
    /// (property access returns undefined), while serialize_struct produces a
    /// plain JS object. This test ensures the JSON structure is a `{...}` object.
    #[test]
    fn lint_diagnostic_serializes_as_json_object() {
        let diag = LintDiagnostic {
            rule: "no-unused-vars".to_string(),
            category: "recommended".to_string(),
            severity: Severity::Warning,
            message: "Variable 'x' is defined but never used.".to_string(),
            span: verter_span::Span::new(42, 50),
            tags: vec![DiagnosticTag::Unnecessary],
            span_kind: DiagnosticSpanKind::ScriptCallSite,
            certainty: Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        };

        let json = serde_json::to_string(&diag).expect("serialize");

        // Must be a JSON object, not an array or other structure
        assert!(
            json.starts_with('{'),
            "serialized output must be a JSON object"
        );
        assert!(json.ends_with('}'), "serialized output must end with }}");

        // Verify all expected fields are present as direct keys
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert!(parsed.is_object(), "parsed value must be an object");
        let obj = parsed.as_object().unwrap();

        assert_eq!(obj.get("rule").unwrap(), "no-unused-vars");
        assert_eq!(obj.get("category").unwrap(), "recommended");
        assert_eq!(obj.get("severity").unwrap(), "warning");
        assert_eq!(
            obj.get("message").unwrap(),
            "Variable 'x' is defined but never used."
        );
        assert_eq!(obj.get("spanStart").unwrap(), 42);
        assert_eq!(obj.get("spanEnd").unwrap(), 50);
        assert_eq!(obj.get("spanKind").unwrap(), "scriptCallSite");
        assert!(obj.get("tags").unwrap().is_array());
    }

    /// Verify that empty tags are NOT included in the output (conditional field).
    #[test]
    fn lint_diagnostic_omits_empty_tags() {
        let diag = LintDiagnostic {
            rule: "test-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Error,
            message: "test".to_string(),
            span: verter_span::Span::new(0, 5),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Attribute,
            certainty: Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        };

        let json = serde_json::to_string(&diag).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        let obj = parsed.as_object().unwrap();

        // Empty tags should NOT appear
        assert!(obj.get("tags").is_none(), "empty tags must be omitted");
        // Other required fields must still be present
        assert!(obj.get("rule").is_some());
        assert!(obj.get("message").is_some());
        assert!(obj.get("spanStart").is_some());
        assert!(obj.get("spanEnd").is_some());
        assert!(obj.get("spanKind").is_some());
    }

    #[test]
    fn certainty_default_is_definite() {
        assert_eq!(Certainty::default(), Certainty::Definite);
    }

    #[test]
    fn certainty_serde_roundtrip() {
        let definite_json = serde_json::to_string(&Certainty::Definite).unwrap();
        assert_eq!(definite_json, "\"definite\"");
        let roundtrip: Certainty = serde_json::from_str(&definite_json).unwrap();
        assert_eq!(roundtrip, Certainty::Definite);

        let partial_json = serde_json::to_string(&Certainty::Partial).unwrap();
        assert_eq!(partial_json, "\"partial\"");
        let roundtrip: Certainty = serde_json::from_str(&partial_json).unwrap();
        assert_eq!(roundtrip, Certainty::Partial);
    }

    #[test]
    fn evidence_snippet_serde_roundtrip() {
        let snippet = EvidenceSnippet {
            context: "const x = ref(0)".to_string(),
            highlight_start: 6,
            highlight_end: 7,
        };
        let json = serde_json::to_string(&snippet).unwrap();
        assert!(json.contains("highlightStart"), "should use camelCase keys");
        assert!(json.contains("highlightEnd"), "should use camelCase keys");
        let roundtrip: EvidenceSnippet = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip, snippet);
    }

    #[test]
    fn related_file_serde_roundtrip() {
        let rf = RelatedFile {
            path: "src/components/Foo.vue".to_string(),
            reason: "Defines the prop type used here".to_string(),
        };
        let json = serde_json::to_string(&rf).unwrap();
        let roundtrip: RelatedFile = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip, rf);
    }

    #[test]
    fn lint_diagnostic_new_fields_default() {
        let diag = LintDiagnostic {
            rule: "test".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "test".to_string(),
            span: verter_span::Span::new(0, 5),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Attribute,
            certainty: Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        };
        assert_eq!(diag.certainty, Certainty::Definite);
        assert!(diag.evidence.is_empty(), "evidence should default to empty");
        assert!(
            diag.related_files.is_empty(),
            "related_files should default to empty"
        );
    }

    #[test]
    fn lint_diagnostic_serde_omits_empty_evidence_and_related() {
        let diag = LintDiagnostic {
            rule: "test".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "test".to_string(),
            span: verter_span::Span::new(0, 5),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Attribute,
            certainty: Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        };
        let json = serde_json::to_string(&diag).unwrap();
        assert!(
            !json.contains("evidence"),
            "empty evidence should be omitted"
        );
        assert!(
            !json.contains("relatedFiles"),
            "empty relatedFiles should be omitted"
        );
        assert!(
            json.contains("certainty"),
            "certainty should always be present"
        );
    }

    #[test]
    fn lint_diagnostic_serde_includes_populated_evidence() {
        let diag = LintDiagnostic {
            rule: "test".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "test".to_string(),
            span: verter_span::Span::new(0, 5),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Attribute,
            certainty: Certainty::Partial,
            evidence: vec![EvidenceSnippet {
                context: "let x = 1".to_string(),
                highlight_start: 4,
                highlight_end: 5,
            }],
            related_files: vec![RelatedFile {
                path: "Foo.vue".to_string(),
                reason: "defines type".to_string(),
            }],
        };
        let json = serde_json::to_string(&diag).unwrap();
        assert!(
            json.contains("evidence"),
            "populated evidence should appear"
        );
        assert!(
            json.contains("relatedFiles"),
            "populated relatedFiles should appear"
        );
        assert!(
            json.contains("\"partial\""),
            "certainty Partial should serialize"
        );
        let roundtrip: LintDiagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip, diag);
    }

    #[test]
    fn severity_hint_serializes() {
        let json = serde_json::to_string(&Severity::Hint).expect("serialize");
        assert_eq!(json, "\"hint\"");
    }

    #[test]
    fn span_kind_serde_roundtrip() {
        let variants = [
            DiagnosticSpanKind::ElementOpenTag,
            DiagnosticSpanKind::FullElement,
            DiagnosticSpanKind::Attribute,
            DiagnosticSpanKind::Directive,
            DiagnosticSpanKind::CssSelector,
            DiagnosticSpanKind::CssClassName,
            DiagnosticSpanKind::Interpolation,
            DiagnosticSpanKind::ConditionExpression,
            DiagnosticSpanKind::ScriptCallSite,
            DiagnosticSpanKind::CrossFileEntry,
            DiagnosticSpanKind::PropDefinition,
            DiagnosticSpanKind::FileLevel,
        ];
        for variant in &variants {
            let json = serde_json::to_string(variant).expect("serialize");
            let roundtrip: DiagnosticSpanKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*variant, roundtrip);
        }
    }
}
