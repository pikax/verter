//! Svelte macro-ordinal walk — the ONE source-order addressing engine for
//! `$props()` / `createEventDispatcher` macro CALLS, plus the depth-closed
//! leaf-member extraction over a lowered props payload.
//!
//! Extracted from `svelte.rs` (same module, sibling file). Both the candidate
//! CAPTURE side (`capture_svelte_candidates`) and the deref-side `lower_*`
//! accessors drive this same walk, so the mint-side and deref-side ordinal
//! conventions cannot drift.

use super::payload::*;
use super::*;

/// One ordinal-bearing macro CALL yielded by the shared [`MacroOrdinalWalk`].
pub(super) enum OrdinalMacroCall<'a, 'b> {
    /// A `$props()` declarator call (`let {…}: T = $props()` /
    /// `let x = $props<T>()`), yielded with its declarator + call init.
    Props {
        declarator: &'b VariableDeclarator<'a>,
        init: &'b Expression<'a>,
    },
    /// A TRACKED `createEventDispatcher` call — the callee's local binding was
    /// imported as `createEventDispatcher`; `import_source` is the module
    /// specifier it was imported from (owned — the walk's tracking list is
    /// walk-internal).
    Dispatcher {
        call: &'b CallExpression<'a>,
        import_source: String,
    },
}

/// The ONE source-order macro-ordinal walk — the shared addressing engine for
/// svelte macro CALLS. Both the candidate CAPTURE (which stamps each payload
/// locator's `macro_index`) and the deref-side position accessor
/// ([`lower_props_annotation_at`]) drive this same walk, so the mint-side and
/// deref-side ordinal conventions cannot drift.
///
/// Convention (each yielded call consumes one ordinal from ONE shared
/// counter — `$props()` runes and tracked dispatcher calls never alias):
///
/// - statements are walked in source order;
/// - a plain variable declaration yields its `$props()` declarators FIRST,
///   then its tracked dispatcher declarators (the deterministic capture
///   order within one statement);
/// - an exported variable declaration yields only its `$props()` declarators,
///   and ONLY in the instance block (a `<script module>` export is a module
///   binding, not a component macro); a whole-statement type-only export
///   yields nothing;
/// - dispatcher-import tracking is source-order INCREMENTAL (a call lexically
///   before its import statement is untracked and consumes no ordinal),
///   matching the single-pass capture semantics.
pub(super) struct MacroOrdinalWalk {
    /// The local binding `createEventDispatcher` was imported under, mapped to
    /// its import source — accumulated as import statements are visited.
    dispatcher_imports: Vec<(String, String)>,
    /// The shared source-order ordinal counter.
    ordinal: u32,
}

impl MacroOrdinalWalk {
    pub(super) fn new() -> Self {
        Self {
            dispatcher_imports: Vec::new(),
            ordinal: 0,
        }
    }

    /// Visit one top-level statement: track its dispatcher imports and yield
    /// its ordinal-bearing macro calls to `visit`.
    pub(super) fn visit_statement<'a, 'b>(
        &mut self,
        stmt: &'b Statement<'a>,
        module_region: Option<(u32, u32)>,
        visit: &mut dyn FnMut(u32, OrdinalMacroCall<'a, 'b>),
    ) {
        match stmt {
            Statement::ImportDeclaration(import) => {
                collect_dispatcher_imports(import, &mut self.dispatcher_imports);
            }
            Statement::VariableDeclaration(decl) => {
                self.visit_props_declarators(&decl.declarations, visit);
                self.visit_dispatcher_declarators(&decl.declarations, visit);
            }
            Statement::ExportNamedDeclaration(export) => {
                // A whole-statement type-only export carries no runtime macro
                // call; a module-block export is a module binding, not a
                // component macro (the same instance-only gate the legacy-prop
                // capture applies).
                if export.export_kind.is_type() {
                    return;
                }
                if statement_in_module(export.span.start, module_region) {
                    return;
                }
                if let Some(Declaration::VariableDeclaration(var)) = &export.declaration {
                    self.visit_props_declarators(&var.declarations, visit);
                }
            }
            _ => {}
        }
    }

    /// Yield each `$props()` declarator, assigning one ordinal per CALL —
    /// whether or not it authors a type payload, so the ordinal stays a pure
    /// call address.
    fn visit_props_declarators<'a, 'b>(
        &mut self,
        declarators: &'b [VariableDeclarator<'a>],
        visit: &mut dyn FnMut(u32, OrdinalMacroCall<'a, 'b>),
    ) {
        for d in declarators {
            let Some(init) = &d.init else { continue };
            if !is_rune_call(init, "$props") {
                continue;
            }
            let ordinal = self.ordinal;
            self.ordinal += 1;
            visit(
                ordinal,
                OrdinalMacroCall::Props {
                    declarator: d,
                    init,
                },
            );
        }
    }

    /// Yield each TRACKED dispatcher declarator (the callee's local binding
    /// was imported as `createEventDispatcher` — an untracked global /
    /// re-export is not provenance-checkable and consumes no ordinal),
    /// assigning one ordinal per tracked CALL whether or not it authors a
    /// type argument.
    fn visit_dispatcher_declarators<'a, 'b>(
        &mut self,
        declarators: &'b [VariableDeclarator<'a>],
        visit: &mut dyn FnMut(u32, OrdinalMacroCall<'a, 'b>),
    ) {
        for d in declarators {
            let Some(init) = &d.init else { continue };
            let Expression::CallExpression(call) = init else {
                continue;
            };
            let Expression::Identifier(ident) = &call.callee else {
                continue;
            };
            // Match the LOCAL binding the dispatcher factory was imported
            // under (handles `import { createEventDispatcher as mk }`).
            let local = ident.name.as_str();
            let Some(import_source) = self
                .dispatcher_imports
                .iter()
                .find(|(binding, _)| binding == local)
                .map(|(_, src)| src.clone())
            else {
                continue;
            };
            let ordinal = self.ordinal;
            self.ordinal += 1;
            visit(
                ordinal,
                OrdinalMacroCall::Dispatcher {
                    call,
                    import_source,
                },
            );
        }
    }
}

/// Depth-closed LEAF extraction over a lowered authored props payload: an
/// inline object literal whose EVERY member value is a closed leaf
/// (primitive / literal / bare argument-less reference) maps to the
/// synthesized leaf-member vocabulary; any deeper shape (nested object,
/// function, union, generic application, index signature) yields `None` —
/// all-or-nothing, so a partial display shape is never fabricated.
fn leaf_members_from_lowered(
    expr: &TypeExpr,
) -> Option<Vec<verter_type_expr::facts::SynthesizedLeafMember>> {
    use verter_type_expr::facts::{LeafTypeFact, SynthesizedLeafMember};
    use verter_type_expr::{LiteralValue, ObjectMember};

    let TypeExpr::Object(obj) = expr else {
        return None;
    };
    let mut members = Vec::with_capacity(obj.properties.len());
    for member in obj.properties.iter() {
        let ObjectMember::Property(property) = member else {
            return None;
        };
        let leaf = match &property.ty {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => LeafTypeFact::Ref(name.as_ref().to_string()),
            TypeExpr::Primitive(name) => LeafTypeFact::Primitive(*name),
            TypeExpr::Literal(LiteralValue::String(text)) => {
                LeafTypeFact::StringLiteral(text.clone())
            }
            TypeExpr::Literal(LiteralValue::Number(value)) => {
                LeafTypeFact::NumberLiteral(value.to_string())
            }
            TypeExpr::Literal(LiteralValue::Boolean(flag)) => LeafTypeFact::BooleanLiteral(*flag),
            _ => return None,
        };
        members.push(SynthesizedLeafMember {
            name: property.name.clone(),
            optional: property.optional,
            ty: leaf,
        });
    }
    Some(members)
}

/// Build the candidate inventory for one walk-yielded macro call.
pub(super) fn capture_ordinal_macro_call(
    call: OrdinalMacroCall<'_, '_>,
    owner: verter_type_expr::TopLevelOwnerId,
    macro_index: u32,
    source: &str,
    snippet_imports: &[(String, String)],
    out: &mut SvelteScriptCandidates,
) {
    match call {
        OrdinalMacroCall::Props {
            declarator: d,
            init,
        } => {
            let mut candidate = SveltePropsCandidate {
                call_span: oxc_span_to_verter(init.span()),
                ..Default::default()
            };
            // 1. `$props<T>()` generic argument (wins over the annotation when
            //    authored, matching the pre-payload-ref capture precedence).
            if let Some(generic_ty) = props_generic_argument_ts_type(init) {
                candidate.props_type = Some(authored_type_payload_ref(
                    generic_ty,
                    source,
                    owner,
                    macro_index,
                    MacroPayloadPosition::TypeArgument,
                ));
                candidate.from_generic_argument = true;
                candidate.props_type_display = type_syntax_display(generic_ty, source);
                candidate.props_type_references =
                    crate::analysis::collect_type_references(generic_ty);
                candidate.props_leaf_members =
                    leaf_members_from_lowered(&lower_ts_type(generic_ty, source));
            }
            // 2. destructuring annotation `let {…}: T = $props()` — the annotation
            //    rides on the DECLARATOR (not the pattern) in this OXC version.
            //    Consulted only when NO generic argument was authored.
            else if let Some(annotation) = &d.type_annotation {
                candidate.props_type = Some(authored_type_payload_ref(
                    &annotation.type_annotation,
                    source,
                    owner,
                    macro_index,
                    MacroPayloadPosition::TypeAnnotation,
                ));
                candidate.props_type_display =
                    type_syntax_display(&annotation.type_annotation, source);
                candidate.props_type_references =
                    crate::analysis::collect_type_references(&annotation.type_annotation);
                candidate.props_leaf_members =
                    leaf_members_from_lowered(&lower_ts_type(&annotation.type_annotation, source));
            }
            // 3. JavaScript/JSDoc binding annotation. Shallow JS analysis
            //    already lowers `@type {T}` through this single sanctioned
            //    text-to-typed-IR boundary; reuse it here and stamp the SAME
            //    TypeAnnotation locator the TS spelling uses. Dereference
            //    replays the JSDoc lowering at this exact macro ordinal.
            else if let Some(jsdoc_type) = extract_jsdoc_type_at_offset(source, d.id.span().start)
            {
                candidate.props_type = Some(authored_type_payload_ref_from_lowered(
                    &jsdoc_type,
                    owner,
                    macro_index,
                    MacroPayloadPosition::TypeAnnotation,
                ));
                candidate.props_leaf_members = leaf_members_from_lowered(&jsdoc_type);
                collect_snippet_candidate_members_from_lowered(&jsdoc_type, snippet_imports, out);
            }
            // 4. `$bindable()` members + prop DEFAULT values from the destructuring
            //    pattern (both are syntax-only reads over the destructuring +
            //    source slice).
            collect_bindable_and_defaults(&d.id, source, &mut candidate);
            // 5. snippet-candidate members from the props type — BOTH the
            //    destructuring annotation (`let {…}: { row: Snippet } = $props()`)
            //    AND the generic argument (`$props<{ row: Snippet }>()`). A member
            //    typed as a `Snippet`-candidate import is recorded (validated later).
            if let Some(annotation) = &d.type_annotation {
                collect_snippet_candidate_members(
                    &annotation.type_annotation,
                    snippet_imports,
                    out,
                );
            }
            if let Some(generic_ty) = props_generic_argument_ts_type(init) {
                collect_snippet_candidate_members(generic_ty, snippet_imports, out);
            }
            out.props = Some(candidate);
        }
        OrdinalMacroCall::Dispatcher {
            call,
            import_source,
        } => {
            if let Some(args) = &call.type_arguments {
                if let Some(first) = args.params.first() {
                    out.dispatcher_events = Some(authored_type_payload_ref(
                        first,
                        source,
                        owner,
                        macro_index,
                        MacroPayloadPosition::TypeArgument,
                    ));
                    out.dispatcher_events_display = type_syntax_display(first, source);
                    out.dispatcher_event_references =
                        crate::analysis::collect_type_references(first);
                    // The import SOURCE is capture inventory: recorded
                    // whenever a type argument is authored
                    // (resolved-validation gates the payload ref on it).
                    out.dispatcher_import_source = Some(import_source);
                }
            }
        }
    }
}
