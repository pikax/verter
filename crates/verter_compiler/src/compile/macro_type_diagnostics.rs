//! Imported-macro-type validation diagnostics.
//!
//! When a `defineProps` / `defineEmits` type argument is a simple reference to
//! an imported type, the bundler/IDE lanes surface a dedicated diagnostic:
//! an UNRESOLVED imported reference degrades softly on the render-only lane
//! (`XUnresolvedImportedMacroType`), while a resolved-but-wrong-shape type is a
//! fatal `XInvalidMacroType` on both lanes. Split out of `compile::mod` so the
//! orchestrator stays under the `no_oversize_files` guard.

use rustc_hash::FxHashSet;

use crate::common::Span;
use crate::diagnostics::{CompilerErrorCode, Diagnostic};
use crate::script::prepared::PreparedScript;
use crate::utils::oxc::script::type_surface::RuntimeType;
use crate::utils::oxc::vue::{MacroTypeParams, ScriptItem, ScriptMacro};

fn is_simple_type_reference(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
}

fn collect_imported_type_names<'a>(items: &'a [ScriptItem<'a>]) -> FxHashSet<&'a str> {
    let mut imported = FxHashSet::default();
    for item in items {
        let ScriptItem::Import(import) = item else {
            continue;
        };
        for binding in &import.bindings {
            if import.is_type_only || binding.is_type_only {
                imported.insert(binding.name);
            }
        }
    }
    imported
}

fn props_type_is_object_like(type_params: &MacroTypeParams) -> bool {
    !type_params.resolved.props.is_empty()
        || type_params
            .resolved
            .root_runtime_types
            .iter()
            .any(|ty| matches!(ty, RuntimeType::Object))
}

fn push_invalid_macro_type_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    code: CompilerErrorCode,
    message: String,
    type_params: &MacroTypeParams,
    content_start: u32,
) {
    // The prepared setup parse runs at the SFC content offset, so `type_span` is
    // SFC-absolute. The public macro-type diagnostic span is content-local
    // (relative to the setup block content): localize it back here so the surfaced
    // span is identical regardless of where the block sits in the SFC.
    let local_span = Span::new(
        type_params.type_span.start - content_start,
        type_params.type_span.end - content_start,
    );
    diagnostics.push(Diagnostic::error_with_message("script", code, message).with_span(local_span));
}

fn validate_imported_macro_type(
    macro_name: &str,
    type_params: &MacroTypeParams,
    type_text: &str,
    imported_type_names: &FxHashSet<&str>,
    content_start: u32,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !is_simple_type_reference(type_text) || !imported_type_names.contains(type_text) {
        return;
    }

    if type_params.unresolved_type_ref {
        // An imported type reference that could not be RESOLVED. Distinct
        // code from the wrong-shape cases below: the render-only bundler
        // lane softens ONLY this (the type degrades to `Unknown`), while a
        // resolved-but-wrong-shape type stays a fatal `XInvalidMacroType`.
        push_invalid_macro_type_diagnostic(
            diagnostics,
            CompilerErrorCode::XUnresolvedImportedMacroType,
            format!(
                "{}() type argument '{}' could not be resolved.",
                macro_name, type_text
            ),
            type_params,
            content_start,
        );
        return;
    }

    match macro_name {
        "defineProps" => {
            if !props_type_is_object_like(type_params) {
                // Resolved-but-wrong-shape: a genuine local misuse. Always
                // fatal, on both lanes.
                push_invalid_macro_type_diagnostic(
                    diagnostics,
                    CompilerErrorCode::XInvalidMacroType,
                    format!(
                        "defineProps() type argument '{}' must resolve to an object-like props type.",
                        type_text
                    ),
                    type_params,
                    content_start,
                );
            }
        }
        "defineEmits" if type_params.resolved.call_signatures.is_empty() => {
            push_invalid_macro_type_diagnostic(
                diagnostics,
                CompilerErrorCode::XInvalidMacroType,
                format!(
                    "defineEmits() type argument '{}' must resolve to emit call signatures or a named-tuple emits object.",
                    type_text
                ),
                type_params,
                content_start,
            );
        }
        _ => {}
    }
}

/// Read a macro type argument's source text from the setup content slice.
///
/// [`MacroTypeParams::type_span`] is SFC-absolute (the prepared setup parse runs
/// at the setup content offset), so it is localized against `content_str` here.
fn macro_type_argument_text<'a>(
    content_str: &'a str,
    content_start: u32,
    type_params: &MacroTypeParams,
) -> &'a str {
    let start = (type_params.type_span.start - content_start) as usize;
    let end = (type_params.type_span.end - content_start) as usize;
    content_str[start..end].trim()
}

pub(super) fn collect_invalid_macro_type_diagnostics(prepared: &PreparedScript) -> Vec<Diagnostic> {
    let Some(setup) = prepared.setup() else {
        return Vec::new();
    };

    // The setup block was parsed once (companion + external types already folded
    // in) when the prepared script was built — read the macro surfaces from that
    // single parse instead of re-parsing and re-resolving here. Macro type-span
    // coordinates are SFC-absolute (the prepared parse runs at the setup content
    // offset), so they are localized against the content slice when read.
    let content_str = setup.content_str();
    let content_start = setup.content_start();
    let parsed_script = setup.parse_result();
    let imported_type_names = collect_imported_type_names(&parsed_script.items);
    let mut diagnostics = Vec::new();

    for item in &parsed_script.items {
        let ScriptItem::Macro(mac) = item else {
            continue;
        };

        match mac {
            ScriptMacro::DefineProps {
                type_params: Some(type_params),
                ..
            } => {
                let type_text = macro_type_argument_text(content_str, content_start, type_params);
                validate_imported_macro_type(
                    "defineProps",
                    type_params,
                    type_text,
                    &imported_type_names,
                    content_start,
                    &mut diagnostics,
                );
            }
            ScriptMacro::WithDefaults {
                define_props_type_params: Some(type_params),
                defaults,
                defaults_arg_span,
                ..
            } => {
                let type_text = macro_type_argument_text(content_str, content_start, type_params);
                let has_defaults_fallback = defaults.is_some() || defaults_arg_span.is_some();
                let skip_unresolved_import_error = has_defaults_fallback
                    && type_params.unresolved_type_ref
                    && is_simple_type_reference(type_text)
                    && imported_type_names.contains(type_text);
                if skip_unresolved_import_error {
                    continue;
                }
                validate_imported_macro_type(
                    "defineProps",
                    type_params,
                    type_text,
                    &imported_type_names,
                    content_start,
                    &mut diagnostics,
                );
            }
            ScriptMacro::DefineEmits {
                type_params: Some(type_params),
                ..
            } => {
                let type_text = macro_type_argument_text(content_str, content_start, type_params);
                validate_imported_macro_type(
                    "defineEmits",
                    type_params,
                    type_text,
                    &imported_type_names,
                    content_start,
                    &mut diagnostics,
                );
            }
            _ => {}
        }
    }

    diagnostics
}

// ── Macro scope-reference validation ──────────────────────────────────────

use oxc_ast::ast::{BindingPattern, Expression, ObjectPropertyKind, PropertyKey, Statement};
use rustc_hash::FxHashMap;

use crate::template::code_gen::binding::BindingType;
use crate::utils::oxc::bindings::collect_expression_free_ref_spans;

/// The official `@vue/compiler-sfc` rejection message template for a
/// setup-scoped macro-argument reference (3.6.0-rc.1; the macro name is
/// substituted in).
fn scope_message(macro_name: &str) -> String {
    format!(
        "`{macro_name}()` in <script setup> cannot reference locally declared variables because it will be hoisted outside of the setup() function. If your component options require initialization in the module scope, use a separate normal <script> to export the options instead."
    )
}

/// `defineProps` / `defineEmits` / `defineOptions` / `defineModel` (and
/// `withDefaults` defaults) scope-reference validation (official
/// `checkInvalidScopeReference`): runtime macro arguments are hoisted outside
/// `setup()`, so a reference to a locally declared (setup-scope) variable
/// breaks at runtime — the official compiler rejects the SFC. Exemptions
/// match official: literal-const bindings (hoistable constants, incl.
/// all-literal enums) and imports (module scope).
pub(super) fn collect_invalid_options_scope_diagnostics(
    prepared: &PreparedScript,
) -> Vec<Diagnostic> {
    let Some(setup) = prepared.setup() else {
        return Vec::new();
    };
    let content_str = setup.content_str();
    let parse_result = setup.parse_result();

    // Name → BindingType map over the SETUP-LOCAL value/import bindings only.
    //
    // The scope check keys names by slicing the content-relative `content_str`,
    // so every span fed in MUST be content-relative. `parse_result.bindings`
    // mixes coordinate systems: the setup value/import declarations
    // (`is_setup()`) carry content-relative spans, but the `Props` family
    // (runtime `defineProps({ x: ... })` keys, `defineProps<T>()` members) carry
    // FILE-relative spans (offset by `content_offset` for downstream template
    // mapping). Feeding those file-relative Props spans into a content-relative
    // slice reads the wrong bytes and can land on — and overwrite — a genuine
    // setup binding of the same length (e.g. a 1-char prop key shifted onto a
    // 1-char `ref` local), silently demoting it to `Props` and skipping the
    // error. `is_setup()` selects exactly the content-relative subset, which is
    // also the only subset the check can flag; the `Props` prop-definition keys
    // are never referenceable setup locals. The `end > len` guard still drops
    // any companion-folded inventory span that lands out of bounds.
    let mut binding_types: FxHashMap<&str, BindingType> = FxHashMap::default();
    for (span, bt) in &parse_result.bindings {
        if !bt.is_setup() {
            continue;
        }
        let (start, end) = (span.start as usize, span.end as usize);
        if end > content_str.len() {
            continue;
        }
        let name = &content_str[start..end];
        binding_types.insert(name, *bt);
    }

    let mut diagnostics = Vec::new();
    // Macro calls at top level — bare statements (defineProps/defineEmits/
    // defineOptions, withDefaults) and assigned declarators (defineModel is
    // `const m = defineModel(...)`; withDefaults wraps defineProps into a
    // const).
    //
    // Official peels wrappers around the CALL before `isCallOf` dispatch:
    // `unwrapTSNode(node.expression)` for a bare `ExpressionStatement` and
    // `unwrapTSNode(decl.init)` for a declarator init (see `compileScript`). Babel
    // additionally folds parentheses into a flag with no wrapper node, whereas OXC
    // materialises an explicit `ParenthesizedExpression`, so `unwrap_ts_node`
    // strips parens plus the 5 TS wrapper nodes. Without the peel a wrapped
    // `(defineProps({ … }))` / `defineProps({ … }) as T` is not recognised as a
    // macro call and the entire scope walk is skipped.
    for stmt in &setup.program().body {
        match stmt {
            Statement::ExpressionStatement(es) => {
                if let Expression::CallExpression(call) = unwrap_ts_node(&es.expression) {
                    check_macro_call(call, &binding_types, content_str, &mut diagnostics);
                }
            }
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    let Some(init) = declarator.init.as_ref().map(|e| unwrap_ts_node(e)) else {
                        continue;
                    };
                    let Expression::CallExpression(call) = init else {
                        continue;
                    };
                    check_macro_call(call, &binding_types, content_str, &mut diagnostics);
                    // A DIRECT `defineProps` reactive-destructure declaration over
                    // an OBJECT pattern (`const { x = <default> } = defineProps(...)`)
                    // hoists its default expressions with the props runtime decl
                    // (the `mergeDefaults` merge), so a default referencing a
                    // setup-local is rejected under `defineProps()` — official
                    // `checkInvalidScopeReference(ctx.propsDestructureDecl, DEFINE_PROPS)`.
                    // Official records `ctx.propsDestructureDecl` ONLY in
                    // `processDefineProps` when `!isWithDefaults` AND
                    // `declId.type === "ObjectPattern"`. Two forms are therefore
                    // NOT props destructures and must not be scope-checked: under
                    // `withDefaults(...)` reactive destructure is disabled (defaults
                    // stay in setup scope), and a top-level ARRAY pattern is never a
                    // props destructure.
                    if let Expression::Identifier(callee) = &call.callee {
                        if callee.name.as_str() == "defineProps"
                            && matches!(&declarator.id, BindingPattern::ObjectPattern(_))
                        {
                            check_destructure_pattern_defaults(
                                &declarator.id,
                                &binding_types,
                                content_str,
                                &mut diagnostics,
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
    diagnostics
}

/// Apply the official scope-reference rule to one top-level macro call.
fn check_macro_call(
    call: &oxc_ast::ast::CallExpression,
    binding_types: &FxHashMap<&str, BindingType>,
    content_str: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Expression::Identifier(callee) = &call.callee else {
        return;
    };
    let callee_name = callee.name.as_str();
    match callee_name {
        "defineProps" | "defineEmits" => {
            // Official checks the runtime declarations — every argument. The
            // whole `defineProps` / `defineEmits` runtime argument is hoisted
            // out of setup(), so a setup-local reference anywhere in it is
            // rejected.
            for arg in &call.arguments {
                if let Some(expr) = arg.as_expression() {
                    check_scope_references(
                        expr,
                        callee_name,
                        binding_types,
                        content_str,
                        diagnostics,
                    );
                }
            }
        }
        "defineModel" => {
            // `defineModel` is NOT hoisted wholesale: its options object's
            // `get`/`set` transformer functions are emitted back INTO setup()
            // (they wrap the model ref via `useModel`), so a setup-local
            // reference inside `get`/`set` is valid. Official scope-checks only
            // the non-`get`/`set` option properties.
            check_define_model_scope_references(call, binding_types, content_str, diagnostics);
        }
        "defineOptions" => {
            // Official checks `optionsRuntimeDecl` (the first argument).
            if let Some(expr) = call.arguments.first().and_then(|a| a.as_expression()) {
                check_scope_references(expr, callee_name, binding_types, content_str, diagnostics);
            }
        }
        "withDefaults" => {
            // Official checks `propsRuntimeDefaults` (the defaults
            // argument) — reported under `defineProps()`.
            if let Some(expr) = call.arguments.get(1).and_then(|a| a.as_expression()) {
                check_scope_references(
                    expr,
                    "defineProps",
                    binding_types,
                    content_str,
                    diagnostics,
                );
            }
        }
        _ => {}
    }
}

/// Scope-check the default expressions of a `defineProps` reactive-destructure
/// pattern.
///
/// The caller enters here ONLY for a direct `defineProps(...)` declaration whose
/// declaration id is an `ObjectPattern` — official's `processPropsDestructure`
/// precondition (`!isWithDefaults && declId.type === "ObjectPattern"`), the sole
/// case that records `ctx.propsDestructureDecl`.
///
/// A destructure declaration's defaults (`const { x = <default>, y: z = <default> }
/// = defineProps(...)`) are hoisted with the props runtime decl (the
/// `mergeDefaults` merge), so a default that references a setup-local breaks at
/// runtime. Official records the whole destructure declaration
/// (`ctx.propsDestructureDecl`) and runs
/// `checkInvalidScopeReference(ctx.propsDestructureDecl, DEFINE_PROPS)`;
/// `walkIdentifiers` flags only the REFERENCED identifiers — the `right` side of
/// each `AssignmentPattern` default — never the destructured binding targets /
/// aliases (`walkDeclaration` does not register a props destructure's names in
/// `setupBindings`). This mirrors that exactly: every default `right` expression
/// is scope-checked under `defineProps`, while binding targets are only recursed
/// through to reach further nested defaults. DIAGNOSTIC ONLY — the
/// reactive-destructure `_mergeDefaults` / `__props` runtime transform is a
/// separate concern and is intentionally NOT implemented here.
fn check_destructure_pattern_defaults(
    pattern: &BindingPattern,
    binding_types: &FxHashMap<&str, BindingType>,
    content_str: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(_) => {}
        BindingPattern::AssignmentPattern(assign) => {
            check_scope_references(
                &assign.right,
                "defineProps",
                binding_types,
                content_str,
                diagnostics,
            );
            check_destructure_pattern_defaults(
                &assign.left,
                binding_types,
                content_str,
                diagnostics,
            );
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                check_destructure_pattern_defaults(
                    &prop.value,
                    binding_types,
                    content_str,
                    diagnostics,
                );
            }
            if let Some(rest) = &obj.rest {
                check_destructure_pattern_defaults(
                    &rest.argument,
                    binding_types,
                    content_str,
                    diagnostics,
                );
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                check_destructure_pattern_defaults(elem, binding_types, content_str, diagnostics);
            }
            if let Some(rest) = &arr.rest {
                check_destructure_pattern_defaults(
                    &rest.argument,
                    binding_types,
                    content_str,
                    diagnostics,
                );
            }
        }
    }
}

/// Apply the official scope-reference rule to a `defineModel` call.
///
/// Official `processDefineModel` (`@vue/compiler-sfc` 3.6.0-rc.1) treats a
/// `defineModel` options object differently from `defineProps` / `defineEmits`:
/// the `get` / `set` transformer functions are emitted back INTO `setup()` (they
/// wrap the model ref through `useModel`), so ONLY the remaining option
/// properties (`default`, `type`, `required`, `validator`, …) are hoisted and
/// collected into `runtimeOptionNodes` for the scope check. A setup-local
/// referenced inside `get` / `set` is therefore valid. If the options object has
/// a spread element or a computed key it cannot be statically analysed, and
/// official collects no runtime option nodes — the whole object is skipped. This
/// mirrors that exactly.
fn check_define_model_scope_references(
    call: &oxc_ast::ast::CallExpression,
    binding_types: &FxHashMap<&str, BindingType>,
    content_str: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Resolve the options object, matching official argument handling:
    //   defineModel(options)          → options = arg0
    //   defineModel("name", options)  → options = arg1
    // A leading string / no-substitution template literal is the model NAME
    // (never scope-checked — a literal carries no free references).
    let Some(arg0) = call.arguments.first().and_then(|a| a.as_expression()) else {
        return;
    };
    let arg0 = unwrap_ts_node(arg0);
    let has_name = match arg0 {
        Expression::StringLiteral(_) => true,
        Expression::TemplateLiteral(tpl) => tpl.expressions.is_empty(),
        _ => false,
    };
    let options = if has_name {
        // Official reads `node.arguments[1]` WITHOUT `unwrapTSNode`, so a
        // TS-wrapped named options object (`({ … } as T)`) is intentionally NOT
        // statically analysed (parity is preserved). OXC, however, materialises
        // `(expr)` as an explicit `ParenthesizedExpression` node, whereas Babel
        // (official's parser) folds parentheses into an `extra.parenthesized`
        // flag with no wrapper node — so a merely-parenthesised options object
        // (`({ … })`) still satisfies official's `options.type === "ObjectExpression"`
        // test. Peel ONLY the paren node (never the TS wrappers) so the named
        // form reaches the same ObjectExpression and the get/set-aware scope
        // check applies instead of being silently skipped.
        call.arguments
            .get(1)
            .and_then(|a| a.as_expression())
            .map(strip_parens)
    } else {
        Some(arg0)
    };
    // Non-object (or absent) options are not statically analysable — official
    // leaves `runtimeOptionNodes` empty and checks nothing.
    let Some(Expression::ObjectExpression(obj)) = options else {
        return;
    };
    // A spread element or a computed key defeats static analysis — official
    // skips the whole options object.
    let has_spread_or_computed = obj.properties.iter().any(|p| match p {
        ObjectPropertyKind::SpreadProperty(_) => true,
        ObjectPropertyKind::ObjectProperty(prop) => prop.computed,
    });
    if has_spread_or_computed {
        return;
    }
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        if property_key_is_get_or_set(&prop.key) {
            // `get` / `set` transformers stay inside setup() — not hoisted.
            continue;
        }
        check_scope_references(
            &prop.value,
            "defineModel",
            binding_types,
            content_str,
            diagnostics,
        );
    }
}

/// Strip the TS wrapper nodes official `unwrapTSNode` peels (`as` / satisfies /
/// non-null / type-assertion / instantiation), plus parentheses, so a wrapped
/// options object (`defineModel({ … } as ModelOptions)`, `defineModel(({ … }))`)
/// is analysed like its inner expression.
fn unwrap_ts_node<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    let mut current = expr;
    loop {
        current = match current {
            Expression::ParenthesizedExpression(inner) => &inner.expression,
            Expression::TSAsExpression(inner) => &inner.expression,
            Expression::TSSatisfiesExpression(inner) => &inner.expression,
            Expression::TSNonNullExpression(inner) => &inner.expression,
            Expression::TSTypeAssertion(inner) => &inner.expression,
            Expression::TSInstantiationExpression(inner) => &inner.expression,
            _ => break,
        };
    }
    current
}

/// Peel `ParenthesizedExpression` wrappers ONLY (never the TS wrapper nodes).
///
/// OXC represents `(expr)` as an explicit `ParenthesizedExpression` node, while
/// Babel — official's parser — records parentheses as an `extra.parenthesized`
/// flag on the inner node and exposes no wrapper. Where official reads a raw
/// argument WITHOUT `unwrapTSNode` (the `defineModel("name", options)` named
/// options arg), a merely-parenthesised value must have its OXC paren node peeled
/// to match Babel, but a TS-wrapped value must be left intact so it is treated
/// as non-statically-analysable exactly as official treats it.
fn strip_parens<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    let mut current = expr;
    while let Expression::ParenthesizedExpression(inner) = current {
        current = &inner.expression;
    }
    current
}

/// Whether an object-property key is `get` or `set` (identifier or string-literal
/// form), matching official's `p.key.name`/`p.key.value` check.
fn property_key_is_get_or_set(key: &PropertyKey) -> bool {
    match key {
        PropertyKey::StaticIdentifier(id) => matches!(id.name.as_str(), "get" | "set"),
        PropertyKey::StringLiteral(s) => matches!(s.value.as_str(), "get" | "set"),
        _ => false,
    }
}

/// Walk every free identifier reference in a macro-argument expression
/// (complete Visit walker — nested calls/member chains included, property
/// keys excluded) and emit the official error for setup-scope references.
fn check_scope_references(
    expr: &Expression,
    macro_name: &str,
    binding_types: &FxHashMap<&str, BindingType>,
    content_str: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut spans = FxHashSet::default();
    collect_expression_free_ref_spans(expr, &FxHashSet::default(), &mut spans);
    for span in &spans {
        let name = &content_str[span.start as usize..span.end as usize];
        let is_setup_local = binding_types.get(name).is_some_and(|bt| {
            bt.is_setup() && *bt != BindingType::LiteralConst && *bt != BindingType::SetupImport
        });
        if is_setup_local {
            // Identifier spans are content-relative (the prepared parse
            // runs over the content slice), already the content-local
            // shape the public diagnostics use.
            let local_span = Span::new(span.start, span.end);
            diagnostics.push(
                Diagnostic::error_with_message(
                    "script",
                    CompilerErrorCode::XInvalidMacroScopeReference,
                    scope_message(macro_name),
                )
                .with_span(local_span),
            );
        }
    }
}
