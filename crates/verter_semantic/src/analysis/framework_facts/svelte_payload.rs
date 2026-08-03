//! Payload construction and stable hashing helpers for Svelte script facts.

use super::*;

/// The content-free anchor of a svelte macro payload: the component's
/// `default` value symbol under the analyzer's local-file convention (the
/// empty producing canonical = the component's own file). Mirrors the Vue
/// analyzer's macro-payload anchor — the owning declaration is the
/// component's synthesized default-export value symbol.
pub(super) fn local_default_anchor(owner: verter_type_expr::TopLevelOwnerId) -> AuthoredAnchor {
    AuthoredAnchor {
        canonical_id: Arc::from(""),
        owner,
        symbol: Arc::from("default"),
        space: LocatorSymbolSpace::Value,
    }
}

/// Whether any captured authored-type payload ref still carries the
/// local-file EMPTY-sentinel anchor (`canonical_id == ""`) — the
/// [`ScriptFactProvider::absolutize_candidates`] no-op fast path predicate.
pub(super) fn carries_empty_payload_anchor(candidates: &SvelteScriptCandidates) -> bool {
    let is_empty = |payload_ref: &AuthoredTypePayloadRef| {
        payload_ref_anchor(&payload_ref.locator)
            .canonical_id
            .is_empty()
    };
    candidates
        .props
        .as_ref()
        .and_then(|p| p.props_type.as_ref())
        .is_some_and(is_empty)
        || candidates.dispatcher_events.as_ref().is_some_and(is_empty)
}

/// The anchor of a payload-ref locator — every locator kind carries exactly
/// one authored anchor.
pub(super) fn payload_ref_anchor(locator: &AuthoredBodyLocator) -> &AuthoredAnchor {
    match locator {
        AuthoredBodyLocator::DeclBody(slot) => &slot.anchor,
        AuthoredBodyLocator::AugmentationBody(aug) => &aug.anchor,
        AuthoredBodyLocator::JsdocTypedefBody(typedef) => &typedef.anchor,
        AuthoredBodyLocator::MacroPayload(payload) => &payload.anchor,
    }
}

/// Fill an EMPTY payload-ref anchor with the producing canonical. A
/// non-empty anchor may be a cross-file resolver's canonical and is never
/// rewritten (the locator contract), which also makes the fill idempotent.
/// The `payload_hash` axis is untouched — it fingerprints the authored TYPE,
/// not the anchor.
pub(super) fn fill_empty_payload_ref_anchor(
    payload_ref: &mut AuthoredTypePayloadRef,
    canonical: &str,
) {
    let anchor = match &mut payload_ref.locator {
        AuthoredBodyLocator::DeclBody(slot) => &mut slot.anchor,
        AuthoredBodyLocator::AugmentationBody(aug) => &mut aug.anchor,
        AuthoredBodyLocator::JsdocTypedefBody(typedef) => &mut typedef.anchor,
        AuthoredBodyLocator::MacroPayload(payload) => &mut payload.anchor,
    };
    if anchor.canonical_id.is_empty() {
        anchor.canonical_id = Arc::from(canonical);
    }
}

/// The authored-type payload REFERENCE of a props / dispatcher type position:
/// a content-free [`MacroPayloadLocator`] (the re-resolution address) plus a
/// parse-stable STRUCTURAL `payload_hash` of the authored type (the cache
/// discriminator).
///
/// NEVER fail-closed: a bare named reference (`Props`), an inline object
/// literal (`{ a: string }`), and an instantiation carrying type arguments
/// (`Props<T>`) all yield a payload ref — the authored payload is carried by
/// POSITION, so no authored shape is lost, and two captures whose authored
/// content differs always discriminate through the hash.
///
/// The authored `TSType` lowers ONCE via `lower_ts_type` into transient
/// producer-local typed IR, is fingerprinted through the shared
/// alpha-normalised semantic hasher ([`compute_semantic_hash`] — span-free,
/// so the content-addressed candidate slot stays stable across
/// formatting-only edits), and is dropped — the lowered `TypeExpr` never
/// leaves this function. Capture is SYNTAX-ONLY, so every named reference
/// hashes as an unresolved reference-shape edge (name + space) via
/// [`UnresolvedLens`] — exactly the content discrimination the candidate slot
/// needs; resolved reference identity stays the fact rail's job. A
/// depth-budget-exceeded walk still yields a deterministic hash (the
/// budget-fold arm of the shared hasher).
pub(super) fn authored_type_payload_ref(
    ty: &TSType<'_>,
    source: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    macro_index: u32,
    payload: MacroPayloadPosition,
) -> AuthoredTypePayloadRef {
    let lowered: TypeExpr = lower_ts_type(ty, source);
    authored_type_payload_ref_from_lowered(&lowered, owner, macro_index, payload)
}

/// Stamp an already-lowered authored payload onto the Svelte macro-position
/// locator. Used by JSDoc, whose tag payload is typed IR rather than a `TSType`
/// AST node, while preserving the same structural hash and dereference path as
/// the TypeScript spelling.
pub(super) fn authored_type_payload_ref_from_lowered(
    lowered: &TypeExpr,
    owner: verter_type_expr::TopLevelOwnerId,
    macro_index: u32,
    payload: MacroPayloadPosition,
) -> AuthoredTypePayloadRef {
    let outcome = compute_semantic_hash(lowered, SymbolSpace::Type, &UnresolvedLens);
    AuthoredTypePayloadRef {
        locator: AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
            anchor: local_default_anchor(owner),
            macro_index,
            payload,
        }),
        payload_hash: outcome.hash,
    }
}

/// The raw `$props<T>()` generic type-argument OXC `TSType`, for snippet-member
/// scanning over the un-lowered annotation.
pub(super) fn props_generic_argument_ts_type<'a>(
    init: &'a Expression<'a>,
) -> Option<&'a TSType<'a>> {
    let Expression::CallExpression(call) = init else {
        return None;
    };
    call.type_arguments.as_ref()?.params.first()
}

/// Whether `expr` is a call to the named rune (`$props` / `$bindable` / …).
pub(super) fn is_rune_call(expr: &Expression<'_>, rune: &str) -> bool {
    if let Expression::CallExpression(call) = expr {
        if let Expression::Identifier(ident) = &call.callee {
            return ident.name == rune;
        }
    }
    false
}

/// The simple binding name of a pattern, when it is a plain identifier.
pub(super) fn binding_name(pattern: &BindingPattern<'_>) -> Option<String> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str().to_string()),
        _ => None,
    }
}

/// The static name of a property/binding key, when it is a plain identifier or
/// string literal.
pub(super) fn property_key_name<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

pub(super) fn oxc_span_to_verter(span: oxc_span::Span) -> Span {
    Span::new(span.start, span.end)
}

/// A structural hash of the captured candidates, invariant under cosmetic edits
/// — lets the content-addressed candidate slot stay stable across formatting.
///
/// Every SEMANTIC capture field folds in DIRECTLY as typed data (the authored
/// payload refs hash their own `Hash` impls — locator + parse-stable
/// payload hash, never a `format!("{:?}", …)` debug rendering); spans
/// deliberately do NOT fold in (they shift under formatting-only edits). The
/// hash shape is versioned by [`SvelteScriptProvider::VERSION`].
pub(super) fn stable_candidate_hash(candidates: &SvelteScriptCandidates) -> [u8; 16] {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    candidates.props.is_some().hash(&mut hasher);
    if let Some(p) = &candidates.props {
        p.from_generic_argument.hash(&mut hasher);
        p.bindable_members.hash(&mut hasher);
        // The payload REF hashes both axes: the locator (authored position)
        // and the parse-stable structural payload hash (authored content) —
        // `$props<{ a: string }>()` and `$props<{ a: number }>()` occupy
        // distinct candidate slots.
        p.props_type.hash(&mut hasher);
        // Defaults are part of the captured shape — an edited default value
        // changes the hash so the content-addressed candidate slot misses.
        for d in &p.prop_defaults {
            d.key.hash(&mut hasher);
            d.value.hash(&mut hasher);
        }
    }
    for c in &candidates.snippet_imports {
        c.imported_name.hash(&mut hasher);
        // The binding FORM discriminates the slot (a statement binding and a
        // binding-less import()-type reference are distinct captured shapes),
        // and a statement's local binding stays part of the shape.
        match &c.binding {
            crate::analysis::framework_facts::svelte::SvelteSnippetCandidateBinding::Statement { local_binding } => {
                1u8.hash(&mut hasher);
                local_binding.hash(&mut hasher);
            }
            crate::analysis::framework_facts::svelte::SvelteSnippetCandidateBinding::ImportTypeReference => {
                2u8.hash(&mut hasher);
            }
        }
        c.import_source.hash(&mut hasher);
    }
    for export in &candidates.instance_exports {
        export.exported_name.hash(&mut hasher);
        export.local_name.hash(&mut hasher);
        export.owner.hash(&mut hasher);
        export.binding_key.hash(&mut hasher);
    }
    for export in &candidates.module_exports {
        export.exported_name.hash(&mut hasher);
        export.local_name.hash(&mut hasher);
        export.owner.hash(&mut hasher);
        export.binding_key.hash(&mut hasher);
    }
    for p in &candidates.legacy_props {
        p.name.hash(&mut hasher);
        p.has_default.hash(&mut hasher);
    }
    candidates.dispatcher_import_source.hash(&mut hasher);
    candidates.dispatcher_events.hash(&mut hasher);
    let h = hasher.finish();
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&h.to_le_bytes());
    out[8..].copy_from_slice(&h.rotate_left(17).to_le_bytes());
    out
}
