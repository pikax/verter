// Cross-file edit support for code actions.
//
// Provides `ChildComponentContext` — a bundle of resolved child component data
// (URI, source, analysis, blocks, line_index) with helper methods for generating
// cross-file `WorkspaceEdit`s that insert macros or type members into child files.

use std::collections::HashSet;

use tower_lsp_server::ls_types::*;
use verter_semantic::analysis::types::{
    AnalysisFlags, AnalyzedMacro, AnalyzedMacroKind, VueApiClassification,
};
use verter_session::FileAnalysisSnapshot;

use crate::documents::carrier_structure::CarrierBlockView;
use crate::documents::line_index::LineIndex;
use crate::features::action_utils;

/// Context for generating edits in a resolved child component.
///
/// Constructed by the server when a parent component references a child via an import.
/// Provides read-only access to the child's analysis data and methods for building
/// cross-file `WorkspaceEdit`s.
#[derive(Clone)]
pub struct ChildComponentContext {
    /// Canonical ID of the child component file.
    pub canonical_id: String,
    /// The URI of the child component file.
    pub uri: Uri,
    /// The full source text of the child component.
    pub source: std::sync::Arc<str>,
    /// The analysis snapshot of the child component.
    pub analysis: FileAnalysisSnapshot,
    /// The child's resolved attribute-fallthrough surface: the attribute names
    /// a parent may pass that the child does not declare. Produced by
    /// `verter_session`'s single inheritance resolver and carried here so the
    /// diagnostic and the code-action layers agree on what is genuinely
    /// unknown — an attribute that falls through must neither be reported nor
    /// offered an "add prop" quick fix. EMPTY = nothing is inherited.
    pub inherited_attrs: std::collections::HashSet<String>,
    /// SFC blocks parsed from the child source.
    pub blocks: Vec<CarrierBlockView>,
    /// Line index for position conversions.
    pub line_index: LineIndex,
}

impl ChildComponentContext {
    /// Find the `<script setup>` block (returns `None` if not present).
    pub fn script_setup(&self) -> Option<&CarrierBlockView> {
        self.blocks.iter().find(|b| b.is_setup())
    }

    /// Find the byte offset to insert a new macro/statement after imports.
    ///
    /// Returns `None` if there is no `<script setup>` block.
    pub fn macro_insert_offset(&self) -> Option<u32> {
        let setup = self.script_setup()?;
        Some(action_utils::find_script_insert_offset(
            &self.source,
            &self.analysis,
            setup,
        ))
    }

    /// Find an existing macro by kind.
    pub fn find_macro(&self, kind: AnalyzedMacroKind) -> Option<&AnalyzedMacro> {
        self.analysis.macros.iter().find(|m| m.kind == kind)
    }

    /// Get the script analysis flags.
    pub fn flags(&self) -> AnalysisFlags {
        AnalysisFlags::from_bits_truncate(self.analysis.script_flags)
    }

    /// Check if `useAttrs()` is called in the child's script.
    pub fn has_use_attrs(&self) -> bool {
        self.analysis
            .vue_api_calls
            .iter()
            .any(|c| c.api == VueApiClassification::UseAttrs)
    }

    /// Check if `defineOptions({ inheritAttrs: false })` is set.
    pub fn has_inherit_attrs_false(&self) -> bool {
        self.flags()
            .contains(AnalysisFlags::HAS_INHERIT_ATTRS_FALSE)
    }

    /// Get defined prop names as a `HashSet`.
    pub fn prop_names(&self) -> HashSet<&str> {
        self.analysis
            .template
            .as_ref()
            .map(|t| t.prop_definitions.iter().map(|p| p.name.as_str()).collect())
            .unwrap_or_default()
    }

    /// Get defined emit names as a `HashSet`.
    pub fn emit_names(&self) -> HashSet<&str> {
        self.analysis
            .template
            .as_ref()
            .map(|t| {
                t.emit_definitions
                    .iter()
                    .filter(|e| e.is_declared)
                    .map(|e| e.event_name.as_str())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Build a `WorkspaceEdit` that inserts text at the macro insert offset.
    ///
    /// Returns `None` if there is no `<script setup>` block.
    pub fn make_insert_at_macros(&self, text: &str) -> Option<WorkspaceEdit> {
        let offset = self.macro_insert_offset()?;
        let position = self.line_index.offset_to_position(offset)?;
        Some(action_utils::make_insert_edit(
            &self.uri,
            position,
            text.to_string(),
        ))
    }

    /// Build a `WorkspaceEdit` that appends a member to an existing macro's
    /// authored type literal in the CHILD file.
    ///
    /// Placement comes from the child's own `type_literal` anchor, so a macro
    /// whose member list is not authored at that position — a runtime macro
    /// (`NotTypeBased`), a bare type reference (`NamedTypeArgument`), an
    /// intersection — yields `None` rather than a guessed offset.
    ///
    /// `ChildComponentContext` pairs a source read and an analysis read taken
    /// independently from the host, so the two are proven to describe the same
    /// bytes before any edit is produced (R5).
    pub fn make_insert_into_macro(
        &self,
        macro_kind: AnalyzedMacroKind,
        text: &str,
    ) -> Option<WorkspaceEdit> {
        let edit_target = action_utils::LiveEditTarget::new(&self.source, &self.line_index);
        if self.analysis.anchor_revision != edit_target.revision() {
            return None;
        }

        let mac = self.find_macro(macro_kind)?;
        let anchor = mac.edit_anchors.type_literal.available()?;
        let position = edit_target.anchor_position(anchor)?;
        Some(action_utils::make_insert_edit(
            &self.uri,
            position,
            text.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::carrier_structure::test_carrier_blocks;
    use verter_semantic::analysis::template::{AnalyzedEmitDefinition, AnalyzedPropDefinition};

    fn make_child_context(source: &str, analysis: FileAnalysisSnapshot) -> ChildComponentContext {
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        ChildComponentContext {
            canonical_id: "/project/src/Child.vue".to_string(),
            uri: "file:///project/src/Child.vue".parse().unwrap(),
            source: source.into(),
            analysis,
            inherited_attrs: std::collections::HashSet::new(),
            blocks,
            line_index,
        }
    }

    #[test]
    fn script_setup_found() {
        let source = "<script setup lang=\"ts\">\nconst x = 1\n</script>";
        let ctx = make_child_context(source, FileAnalysisSnapshot::default());
        assert!(ctx.script_setup().is_some(), "should find <script setup>");

        // Negative: <script> without setup
        let source2 = "<script>\nexport default {}\n</script>";
        let ctx2 = make_child_context(source2, FileAnalysisSnapshot::default());
        assert!(
            ctx2.script_setup().is_none(),
            "should not find script setup in non-setup script"
        );
    }

    #[test]
    fn macro_insert_offset_after_imports() {
        let source =
            "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst x = 1\n</script>";
        let analysis = FileAnalysisSnapshot {
            imports: vec![verter_semantic::analysis::AnalyzedImport {
                source: "vue".into(),
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
                is_type_only: false,
                bindings: vec![],
                span: verter_span::Span::new(24, 49),
                resolved_canonical_id: None,
            }],
            ..Default::default()
        };
        let ctx = make_child_context(source, analysis);

        let offset = ctx.macro_insert_offset().unwrap();
        // Positive: should be at or past the import span_end (49)
        assert!(
            offset >= 49,
            "offset ({offset}) should be at or past the import span_end (49)"
        );
        // Negative: should NOT be at file start
        assert!(offset > 0);
    }

    #[test]
    fn macro_insert_offset_none_without_setup() {
        let source = "<script>\nexport default {}\n</script>";
        let ctx = make_child_context(source, FileAnalysisSnapshot::default());
        assert!(
            ctx.macro_insert_offset().is_none(),
            "no insert offset without <script setup>"
        );
    }

    #[test]
    fn find_macro_by_kind() {
        let analysis = FileAnalysisSnapshot {
            macros: (vec![AnalyzedMacro {
                edit_anchors: Default::default(),
                kind: AnalyzedMacroKind::DefineProps,
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
                is_type_based: true,
                type_references: vec![],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![],
                emit_fields: vec![],
                slot_fields: vec![],
                default_keys: vec![],
                expose_fields: vec![],
                default_values: Vec::new(),
                resolved_local_types: Vec::new(),
                parsed_type_argument: None,
                parsed_type_argument_scope: None,
                span: verter_span::Span::new(24, 60),
            }])
            .into(),
            ..Default::default()
        };
        let source = "<script setup>\ndefineProps<{ msg: string }>()\n</script>";
        let ctx = make_child_context(source, analysis);

        assert!(
            ctx.find_macro(AnalyzedMacroKind::DefineProps).is_some(),
            "should find DefineProps"
        );
        // Negative: DefineEmits is not present
        assert!(
            ctx.find_macro(AnalyzedMacroKind::DefineEmits).is_none(),
            "should not find DefineEmits"
        );
    }

    #[test]
    fn prop_names_returns_all_defined_props() {
        let analysis = FileAnalysisSnapshot {
            template: Some(
                (verter_semantic::analysis::template::TemplateAnalysisSnapshot {
                    prop_definitions: vec![
                        AnalyzedPropDefinition {
                            name: "msg".into(),
                            callable_role: verter_type_expr::PropCallableRole::default(),
                            type_annotation: Some("string".into()),
                            has_default: false,
                            is_required: true,
                            is_boolean: false,
                            used_in_template: true,
                            used_in_script: false,
                            span: verter_span::Span::new(0, 0),
                        },
                        AnalyzedPropDefinition {
                            name: "count".into(),
                            callable_role: verter_type_expr::PropCallableRole::default(),
                            type_annotation: Some("number".into()),
                            has_default: true,
                            is_required: false,
                            is_boolean: false,
                            used_in_template: false,
                            used_in_script: true,
                            span: verter_span::Span::new(0, 0),
                        },
                    ],
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        };
        let source = "<script setup>\ndefineProps<{ msg: string, count: number }>()\n</script>";
        let ctx = make_child_context(source, analysis);

        let names = ctx.prop_names();
        assert!(names.contains("msg"), "should contain msg");
        assert!(names.contains("count"), "should contain count");
        // Negative: does NOT contain undefined props
        assert!(!names.contains("title"), "should not contain title");
    }

    #[test]
    fn emit_names_returns_declared_emits() {
        let analysis = FileAnalysisSnapshot {
            template: Some(
                (verter_semantic::analysis::template::TemplateAnalysisSnapshot {
                    emit_definitions: vec![
                        AnalyzedEmitDefinition {
                            event_name: "save".into(),
                            has_validator: false,
                            is_declared: true,
                            emit_locations: vec![],
                            span: verter_span::Span::new(0, 0),
                        },
                        AnalyzedEmitDefinition {
                            event_name: "delete".into(),
                            has_validator: false,
                            is_declared: false,
                            emit_locations: vec![],
                            span: verter_span::Span::new(0, 0),
                        },
                    ],
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        };
        let source = "<script setup>\ndefineEmits(['save'])\n</script>";
        let ctx = make_child_context(source, analysis);

        let names = ctx.emit_names();
        assert!(names.contains("save"), "should contain declared emit");
        // Negative: undeclared emits are not included
        assert!(
            !names.contains("delete"),
            "should not contain undeclared emit"
        );
    }

    #[test]
    fn has_use_attrs_detects_call() {
        let analysis = FileAnalysisSnapshot {
            vue_api_calls: (vec![verter_semantic::analysis::types::VueApiCallSite {
                api: VueApiClassification::UseAttrs,
                span: verter_span::Span::new(30, 42),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }])
            .into(),
            ..Default::default()
        };
        let source = "<script setup>\nconst attrs = useAttrs()\n</script>";
        let ctx = make_child_context(source, analysis);
        assert!(ctx.has_use_attrs(), "should detect useAttrs()");

        // Negative: no useAttrs
        let ctx2 = make_child_context(source, FileAnalysisSnapshot::default());
        assert!(
            !ctx2.has_use_attrs(),
            "should not detect useAttrs when absent"
        );
    }

    #[test]
    fn make_insert_at_macros_builds_workspace_edit() {
        let source =
            "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst x = 1\n</script>";
        let analysis = FileAnalysisSnapshot {
            imports: vec![verter_semantic::analysis::AnalyzedImport {
                source: "vue".into(),
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
                is_type_only: false,
                bindings: vec![],
                span: verter_span::Span::new(24, 49),
                resolved_canonical_id: None,
            }],
            ..Default::default()
        };
        let ctx = make_child_context(source, analysis);

        let edit = ctx.make_insert_at_macros("defineProps<{ foo: string }>()\n");
        assert!(edit.is_some(), "should produce a workspace edit");

        let edit = edit.unwrap();
        if let Some(DocumentChanges::Edits(doc_edits)) = &edit.document_changes {
            // Positive: URI matches child
            assert_eq!(
                doc_edits[0].text_document.uri.as_str(),
                "file:///project/src/Child.vue"
            );
            // Negative: URI is NOT placeholder sentinel
            assert_ne!(
                doc_edits[0].text_document.uri.as_str(),
                action_utils::SAME_FILE_URI
            );
        } else {
            panic!("expected DocumentChanges::Edits");
        }
    }

    /// A child analysis whose macros AND edit anchors come from the real
    /// analyzer over the child's own source, stamped with that source's
    /// revision — the shape `resolve_component_context` produces.
    fn producer_backed_child_analysis(source: &str) -> FileAnalysisSnapshot {
        let script = crate::features::macro_fixture::analyze_sfc_script(source);
        FileAnalysisSnapshot {
            imports: script.imports.clone(),
            bindings: script.bindings.clone(),
            macros: script.macros.clone().into(),
            script_flags: script.flags.bits(),
            anchor_revision: verter_session::AnalysisSourceRevision::of_source(source),
            ..Default::default()
        }
    }

    /// Apply the single insertion edit to `source`.
    fn apply_child_edit(source: &str, line_index: &LineIndex, edit: &WorkspaceEdit) -> String {
        let Some(DocumentChanges::Edits(doc_edits)) = &edit.document_changes else {
            panic!("expected DocumentChanges::Edits");
        };
        let OneOf::Left(te) = &doc_edits[0].edits[0] else {
            panic!("expected a TextEdit");
        };
        let offset = line_index
            .position_to_offset(&te.range.start)
            .expect("edit position maps back to a byte offset") as usize;
        format!("{}{}{}", &source[..offset], te.new_text, &source[offset..])
    }

    #[test]
    fn make_insert_into_macro_targets_type_literal() {
        let source = "<script setup lang=\"ts\">\ndefineProps<{\n  msg: string\n}>()\n</script>";
        let ctx = make_child_context(source, producer_backed_child_analysis(source));

        let edit = ctx
            .make_insert_into_macro(AnalyzedMacroKind::DefineProps, "  count: number\n")
            .expect("should produce edit for existing type-based macro");
        assert_eq!(
            apply_child_edit(source, &ctx.line_index, &edit),
            "<script setup lang=\"ts\">\ndefineProps<{\n  msg: string\n  count: number\n}>()\n</script>",
            "the member must land before the type literal's closing delimiter"
        );

        // Negative: non-existent macro returns None
        let edit2 =
            ctx.make_insert_into_macro(AnalyzedMacroKind::DefineEmits, "  (e: 'save'): void\n");
        assert!(edit2.is_none(), "should return None for absent macro");
    }

    /// A7-02, cross-file arm: the offset comes from the CHILD's anchor, which
    /// stays exact where `span.end - 4` moved with the macro's trailing text.
    #[test]
    fn cross_file_child_macro_insert_offset_comes_from_child_anchor() {
        let source =
            "<script setup lang=\"ts\">\ndefineProps<{\n  msg: string\n} /* keep */>()\n</script>";
        let ctx = make_child_context(source, producer_backed_child_analysis(source));

        let edit = ctx
            .make_insert_into_macro(AnalyzedMacroKind::DefineProps, "  count: number\n")
            .expect("a type-literal type argument is appendable");
        assert_eq!(
            apply_child_edit(source, &ctx.line_index, &edit),
            "<script setup lang=\"ts\">\ndefineProps<{\n  msg: string\n  count: number\n} /* keep */>()\n</script>",
            "pre-change `span.end - 4` landed inside `/* keep */`"
        );
    }

    /// A7-04, cross-file arm: a bare type reference is fail-closed. Pre-change
    /// `span.end - 4` inserted the member INSIDE the identifier `Props`.
    #[test]
    fn make_insert_into_macro_none_for_named_type_argument() {
        let source = "<script setup lang=\"ts\">\ntype Props = { msg: string }\ndefineProps<Props>()\n</script>";
        let ctx = make_child_context(source, producer_backed_child_analysis(source));

        assert!(
            ctx.make_insert_into_macro(AnalyzedMacroKind::DefineProps, "  count: number\n")
                .is_none(),
            "the member list lives in another declaration ⇒ no edit, never a guessed offset"
        );
    }

    /// R5: `ChildComponentContext` pairs the child's analysis and the child's
    /// source from two INDEPENDENT host reads, so a mismatch produces no edit.
    ///
    /// The two fixtures are the same byte LENGTH and differ only outside the
    /// macro, so the stale anchor is in-bounds and on a character boundary: the
    /// bounds/char-boundary guard cannot catch this, only the revision gate can.
    /// This is the silent-miscarry class, and F2 forbids the edit regardless of
    /// whether the offset happens to still be correct.
    #[test]
    fn make_insert_into_macro_none_when_child_revision_differs() {
        let analyzed =
            "<script setup lang=\"ts\">\ndefineProps<{ msg: string }>()\n// aaa\n</script>";
        let stored =
            "<script setup lang=\"ts\">\ndefineProps<{ msg: string }>()\n// bbb\n</script>";
        assert_eq!(stored.len(), analyzed.len(), "fixtures must match length");
        assert_ne!(stored, analyzed, "fixtures must differ in content");

        let analysis = producer_backed_child_analysis(analyzed);
        let anchor_offset = analysis.macros[0]
            .edit_anchors
            .type_literal
            .available()
            .expect("the fixture mints an available anchor")
            .insert_offset() as usize;
        assert!(
            anchor_offset < stored.len() && stored.is_char_boundary(anchor_offset),
            "the stale anchor must be in-bounds and on a boundary in the stored bytes, \
             so only the revision gate can refuse it"
        );

        let ctx = make_child_context(stored, analysis);
        assert!(
            ctx.make_insert_into_macro(AnalyzedMacroKind::DefineProps, "  count: number\n")
                .is_none(),
            "an analysis paired with different child bytes must produce no edit"
        );

        // Control: the same analysis against ITS OWN bytes does produce one.
        let matched = make_child_context(analyzed, producer_backed_child_analysis(analyzed));
        assert!(
            matched
                .make_insert_into_macro(AnalyzedMacroKind::DefineProps, "  count: number\n")
                .is_some(),
            "control: matching bytes serve the edit"
        );
    }

    #[test]
    fn make_insert_into_macro_none_for_runtime_macro() {
        let source = "<script setup>\ndefineProps(['msg'])\n</script>";
        let ctx = make_child_context(source, producer_backed_child_analysis(source));

        let edit = ctx.make_insert_into_macro(AnalyzedMacroKind::DefineProps, "  foo: string\n");
        // Runtime-based macro can't have type members inserted
        assert!(
            edit.is_none(),
            "should return None for runtime (non-type-based) macro"
        );
    }
}
