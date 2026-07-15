//! The DECLARATOR-SHAPE probes shared by the instance-script item allowlist
//! and the state scan: the assignable effect-family EXPRESSION-rune shape
//! (`$effect.root(fn)` / `$effect.tracking()` declarator inits) and the
//! `$props.id()` declarator shape (the hoisted-id carrier + its literal-only
//! siblings). Extracted from `instance_items.rs` (the file-size guard
//! boundary); both classifiers stay the SINGLE shape authority their consumers
//! (`classify_instance_variable_decl`, `state_scan`) share.

use oxc_ast::ast::{BindingPattern, Comment, VariableDeclarationKind};
use oxc_span::GetSpan;

use super::expr::{call_internal_comment_trivia, carrier_tail_comment_trivia};
use super::instance_items::{init_is_literal_only, SupportedInstanceScriptItem};

/// The typed shape facts of a declaration matching the assignable
/// effect-rune-init carrier SHAPE — the [`effect_rune_init_shape`] output.
pub(super) struct EffectRuneInitShape {
    /// Whether the declaration keyword is `const` (else `let`).
    pub(super) const_decl: bool,
    /// The declared (plain, non-`$`-prefixed) binding name.
    pub(super) name: String,
    /// Which assignable family member the init calls (`EffectRoot` /
    /// `EffectTracking`).
    pub(super) kind: super::expr::EffectFamilyCallKind,
    /// The init CALL expression's span (for the carrier's source slice).
    pub(super) call_span: oxc_span::Span,
    /// The WHOLE init expression's span — transparent author-paren wrappers
    /// included. The carrier slices only `call_span` (the wrapper parens
    /// normalize away), so comments inside the peeled wrapper HEAD range
    /// `[init_span.start, call_span.start)` pre-render into the carrier's
    /// `head_trivia` instead of being sliced away.
    pub(super) init_span: oxc_span::Span,
}

/// The SINGLE declaration-shape predicate of the assignable effect-rune-init
/// carrier: a `let`/`const` declaration (a `var` keeps the var-declaration
/// refusal) with exactly ONE declarator, a plain non-`$`-prefixed identifier
/// binding, no TS annotation, and a WELL-FORMED `$effect.root(fn)` /
/// `$effect.tracking()` call init (the shared family classifier proves the
/// form). BOTH the item carrier ([`SupportedInstanceScriptItem::EffectRuneInit`]
/// via `classify_effect_rune_init`) and the rune-binding fact minting
/// ([`super::state_scan::collect_rune_bindings`]'s `EffectTrackingConst` arm)
/// consult THIS predicate, so the minting can never be broader than the carrier
/// — a declaration the carrier refuses mints no binding fact.
pub(super) fn effect_rune_init_shape(
    decl: &oxc_ast::ast::VariableDeclaration<'_>,
) -> Option<EffectRuneInitShape> {
    use super::expr::EffectFamilyCallKind;
    let const_decl = match decl.kind {
        VariableDeclarationKind::Const => true,
        VariableDeclarationKind::Let => false,
        _ => return None,
    };
    let [d] = decl.declarations.as_slice() else {
        return None;
    };
    if d.type_annotation.is_some() || d.definite {
        return None;
    }
    let BindingPattern::BindingIdentifier(id) = &d.id else {
        return None;
    };
    if id.name.as_str().starts_with('$') {
        return None;
    }
    // The init classifies through the SHARED expression-level family classifier
    // (author parens around the whole init are transparent; the sliced span is
    // the CALL span, so the parens never enter the carrier).
    let init_expr = d.init.as_ref()?;
    let fact = super::expr::effect_family_expression_fact(init_expr)?;
    if !fact.well_formed
        || !matches!(
            fact.kind,
            EffectFamilyCallKind::EffectRoot | EffectFamilyCallKind::EffectTracking
        )
    {
        return None;
    }
    Some(EffectRuneInitShape {
        const_decl,
        name: id.name.to_string(),
        kind: fact.kind,
        call_span: fact.call_span,
        init_span: init_expr.span(),
    })
}

/// Classify a `let`/`const` declarator whose init is a WELL-FORMED assignable
/// effect-family EXPRESSION rune (`$effect.root(fn)` / `$effect.tracking()`) into
/// the [`SupportedInstanceScriptItem::EffectRuneInit`] carrier, or `None` when the
/// declaration is not that shape (the caller falls through to the existing
/// gates). The shape decision is the shared [`effect_rune_init_shape`] predicate;
/// comment trivia inside a transparent wrapper's HEAD range (`= (/*c*/
/// $effect.root(fn))`) pre-renders into the carrier's `head_trivia`, and the
/// carrier TAIL — the LEXICAL trailing region collected by
/// [`carrier_tail_comment_trivia`]: the declaration span's interior after the
/// call end (a normalized wrapper's interior, `= ($effect.root(fn)
/// /*!license*/);`, and an unwrapped init's pre-`;` trailing comments) PLUS
/// the same-line ASI extension of a semicolon-less declaration (`const s =
/// $effect.root(fn) /*!license*/` — the OXC declaration span ends AT the init
/// end, so the trailing comment lies outside every AST span) — into
/// `tail_trivia`, so the carrier slice never silently drops either.
pub(super) fn classify_effect_rune_init(
    decl: &oxc_ast::ast::VariableDeclaration<'_>,
    instance_source: &str,
    comments: &[Comment],
) -> Option<SupportedInstanceScriptItem> {
    let shape = effect_rune_init_shape(decl)?;
    let init = instance_source
        .get(shape.call_span.start as usize..shape.call_span.end as usize)?
        .to_string();
    let head_trivia = call_internal_comment_trivia(
        instance_source,
        comments,
        shape.init_span.start,
        shape.call_span.start,
    );
    let tail_trivia = carrier_tail_comment_trivia(
        instance_source,
        comments,
        shape.call_span.end,
        decl.span.end,
    );
    Some(SupportedInstanceScriptItem::EffectRuneInit {
        const_decl: shape.const_decl,
        name: shape.name,
        init,
        head_trivia,
        tail_trivia,
    })
}

/// The typed shape facts of a declaration matching the `$props.id()` declarator
/// carrier — the [`props_id_decl_shape`] output.
pub(super) struct PropsIdDeclShape {
    /// The declared id binding name.
    pub(super) name: String,
    /// Whether the declaration keyword is `const` (else `let`).
    pub(super) const_decl: bool,
    /// The literal-only sibling declarators, in source order, as
    /// `(name, init span)` rows (`None` = a bare no-init declarator).
    pub(super) siblings: Vec<(String, Option<oxc_span::Span>)>,
}

/// The SINGLE declaration-shape predicate of the `$props.id()` declarator
/// carrier: a `let`/`const` declaration (a `var` keeps the non-`let`
/// rune-declarator refusal) containing EXACTLY ONE declarator whose init is a
/// WELL-FORMED `$props.id()` call (the strict shared
/// [`is_well_formed_props_id_call`](super::expr::is_well_formed_props_id_call)
/// spelling — no parens, no optional chaining, zero args) on a plain
/// non-`$`-prefixed identifier binding with no TS annotation; every OTHER
/// declarator must be a plain non-`$`-prefixed identifier with no TS annotation
/// and a LITERAL-ONLY init (or no init) — the verbatim-emittable sibling subset.
/// BOTH the item carrier and the rune-binding fact minting
/// ([`super::state_scan::collect_rune_bindings`]'s `PropsIdConst` arm) consult
/// THIS predicate, so the minting can never be broader than the carrier. A
/// declaration with TWO `$props.id()` declarators returns `None` (the scan owns
/// the `props_duplicate` refusal).
pub(super) fn props_id_decl_shape(
    decl: &oxc_ast::ast::VariableDeclaration<'_>,
) -> Option<PropsIdDeclShape> {
    use oxc_span::GetSpan;
    let const_decl = match decl.kind {
        VariableDeclarationKind::Const => true,
        VariableDeclarationKind::Let => false,
        _ => return None,
    };
    let mut name: Option<String> = None;
    let mut siblings = Vec::new();
    for d in &decl.declarations {
        let BindingPattern::BindingIdentifier(id) = &d.id else {
            return None;
        };
        if id.name.as_str().starts_with('$') {
            return None;
        }
        if d.init
            .as_ref()
            .is_some_and(|init| super::expr::is_well_formed_props_id_call(init))
        {
            if name.is_some() {
                // A second `$props.id()` declarator in ONE declaration — not the
                // carrier shape (the scan refuses the duplicate use).
                return None;
            }
            name = Some(id.name.to_string());
            continue;
        }
        // A sibling declarator: literal-only init (or none), emitted verbatim.
        match &d.init {
            None => siblings.push((id.name.to_string(), None)),
            Some(init) if init_is_literal_only(init) => {
                siblings.push((id.name.to_string(), Some(init.span())));
            }
            Some(_) => return None,
        }
    }
    Some(PropsIdDeclShape {
        name: name?,
        const_decl,
        siblings,
    })
}

/// Classify a `let`/`const` declaration containing a WELL-FORMED `$props.id()`
/// declarator (plus literal-only siblings) into the
/// [`SupportedInstanceScriptItem::PropsIdDecl`] carrier, or `None` when the
/// declaration is not that shape (the caller falls through to the existing
/// gates). The shape decision is the shared [`props_id_decl_shape`] predicate;
/// the sibling init spans slice their verbatim source text here.
pub(super) fn classify_props_id_decl(
    decl: &oxc_ast::ast::VariableDeclaration<'_>,
    instance_source: &str,
) -> Option<SupportedInstanceScriptItem> {
    let shape = props_id_decl_shape(decl)?;
    let siblings = shape
        .siblings
        .into_iter()
        .map(|(name, span)| {
            let init = span.and_then(|s| {
                instance_source
                    .get(s.start as usize..s.end as usize)
                    .map(str::to_string)
            });
            (name, init)
        })
        .collect();
    Some(SupportedInstanceScriptItem::PropsIdDecl {
        name: shape.name,
        const_decl: shape.const_decl,
        siblings,
    })
}
