//! The SINGLE Svelte compile-options resolver — the one fold point that folds the
//! compile-option side ([`SvelteRuntimeOptions`]) with the inline `<svelte:options>`
//! attributes into the typed [`ResolvedSvelteCompileOptions`] every downstream
//! consumer (root factory, whitespace cleaner, comment serialization, import plan)
//! reads.
//!
//! INLINE WINS per admitted key: an inline `<svelte:options namespace="html">` /
//! `preserveWhitespace` OVERRIDES the corresponding compile option (matching the
//! official `svelte@5.56.3` precedence). The resolver reads the TYPED AST only — the
//! inline value is read through the shared parser value authority
//! ([`options_namespace_value`] / [`options_boolean_value`]), never a raw rescan.
//!
//! FAIL-CLOSED: an officially-ACCEPTED option this backend does not support
//! (`compatibility.componentApi` ≠ `5` / `hmr` / `accessors` / `immutable`) is a typed
//! [`UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported`] refusal (NO runtime
//! module) — EXPLICIT presence rejects, including a `false` / default-equivalent
//! value and a value later masked by an inline override. This is a FEATURE refusal,
//! NOT an official compile-error (the official compiler accepts these).
//!
//! NAMESPACE is html-only: this backend emits html-namespace roots ONLY. A
//! `namespace: 'svg' | 'mathml'` selection (the compile option OR an inline
//! `<svelte:options namespace="svg">`) fails closed with a typed
//! [`UnsupportedSvelteRuntimeSurface::NamespaceUnsupported`] refusal — svg / mathml
//! root element emission is a separate deferred surface, so the resolver refuses a
//! non-`html` namespace rather than resolving to one. A successful resolution is
//! therefore always html-namespaced (the `namespace` axis carries no resolved value).

use crate::svelte::parser::tokenizer::{options_boolean_value, options_namespace_value};
use crate::svelte::parser::{
    ParsedSvelte, SvelteAttribute, SvelteAttributeKind, SvelteAttributeValue,
};
use verter_span::Span;

use super::official_rule::{CoreOfficialValidationRule, OfficialRejection};
use super::parse_refusal::{collect_options_elements, options_attr_name};
use super::unsupported::{CompileOptionOrigin, UnsupportedSvelteCompileOption};
use super::{SvelteRuntimeOptions, UnsupportedSvelteRuntimeSurface};

/// The DOM namespace the root template is created in — the official Svelte
/// `Namespace` (`'html' | 'svg' | 'mathml'`). This backend emits html-namespace
/// roots only; the [`Svg`](SvelteNamespace::Svg) / [`Mathml`](SvelteNamespace::Mathml)
/// variants exist ONLY to carry the parsed selection to the resolver, which fails
/// closed on a non-`html` namespace (svg / mathml element emission is a separate
/// deferred surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteNamespace {
    /// The default HTML namespace (`$.from_html`).
    Html,
    /// The SVG namespace (a fail-closed selection — no emission).
    Svg,
    /// The MathML namespace (a fail-closed selection — no emission).
    Mathml,
}

/// The template-instantiation strategy — the official `fragments` compile option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteFragments {
    /// The default HTML-string clone (`$.from_html`).
    Html,
    /// The CSP-safe tree clone (`$.from_tree`).
    Tree,
}

/// The fully-resolved Svelte compile options — the typed fold of the compile-option
/// side and the inline `<svelte:options>` attributes that every downstream consumer
/// reads. Resolved ONCE per compile request by [`resolve_svelte_compile_options`].
///
/// The `namespace` axis carries NO resolved value: a successful resolution is always
/// html-namespaced (a non-`html` namespace fails closed at the resolver). The
/// component name (`derive_component_name`) and the `cssHash` override are read
/// directly from [`SvelteRuntimeOptions`] by their own consumers (lowering / the
/// style plan), not threaded through here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSvelteCompileOptions {
    /// The resolved fragments strategy (default [`SvelteFragments::Html`]).
    pub fragments: SvelteFragments,
    /// Whether insignificant whitespace text nodes are preserved (default `false`).
    pub preserve_whitespace: bool,
    /// Whether HTML comments are retained in the static skeleton (default `false`).
    pub preserve_comments: bool,
    /// Whether the `svelte/internal/disclose-version` side-effect import is emitted
    /// (default `true`).
    pub disclose_version: bool,
}

/// Resolve the Svelte compile options: fold the compile-option side (`opts`) with the
/// inline `<svelte:options>` attributes (INLINE WINS per admitted key), OR fail closed
/// with a typed [`UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported`] when an
/// unsupported option is explicitly present.
///
/// Runs AFTER the official-reject gate, so the `<svelte:options>` element it reads is
/// already official-ACCEPTED (every malformed form — a duplicate / nested placement,
/// child content, a spread / directive, an invalid `namespace` / `css`, a non-boolean
/// `runes`, an unknown attribute, the deprecated `tag` — was rejected upstream with its
/// exact official code). So the resolver only ever folds valid values and only ever
/// fails closed on the four unsupported feature options.
///
/// The fail-closed order is deterministic: inline `immutable` / `accessors` (in source
/// order) take precedence over the compile-option side, which is then checked in the
/// order `compatibility.componentApi` (any explicit value other than `5`), `hmr`,
/// `accessors`, `immutable`.
pub fn resolve_svelte_compile_options(
    source: &str,
    parsed: &ParsedSvelte,
    opts: &SvelteRuntimeOptions,
) -> Result<ResolvedSvelteCompileOptions, UnsupportedSvelteRuntimeSurface> {
    // The FIRST root `<svelte:options>` element (depth 0), if any. A nested / duplicate
    // options element is an official reject caught upstream, so only the root one folds.
    let mut found = Vec::new();
    collect_options_elements(&parsed.template, 0, &mut found);
    let root_options = found
        .into_iter()
        .find(|(_, depth)| *depth == 0)
        .map(|(el, _)| el);

    // Fold the inline attributes (INLINE WINS). An inline `immutable` / `accessors`
    // fails closed on the FIRST occurrence in source order (before its value matters).
    let mut inline_namespace: Option<SvelteNamespace> = None;
    let mut inline_preserve_ws: Option<bool> = None;
    // The `<svelte:options>` open-tag span, carried for an inline-origin refusal.
    let options_span: Option<Span> = root_options.map(|el| el.open_span);
    if let Some(el) = root_options {
        for attr in &el.attributes {
            let Some(name) = options_attr_name(attr) else {
                // A spread / directive on `<svelte:options>` is an official reject
                // caught upstream; defensively ignored here.
                continue;
            };
            match name.as_str() {
                "namespace" => {
                    inline_namespace = plain_attr_value(attr)
                        .and_then(|value| options_namespace_value(source, value))
                        .map(map_namespace);
                }
                "preserveWhitespace" => {
                    inline_preserve_ws = plain_attr_value(attr)
                        .and_then(|value| options_boolean_value(source, value));
                }
                "immutable" => {
                    return Err(UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported {
                        option: UnsupportedSvelteCompileOption::Immutable,
                        origin: CompileOptionOrigin::Inline,
                        span: Some(el.open_span),
                    });
                }
                "accessors" => {
                    return Err(UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported {
                        option: UnsupportedSvelteCompileOption::Accessors,
                        origin: CompileOptionOrigin::Inline,
                        span: Some(el.open_span),
                    });
                }
                "tag" => {
                    // DEFENSIVE arm (unreachable-by-contract): `<svelte:options tag>`
                    // is the parser-first `svelte_options_deprecated_tag` hard error
                    // (rejected upstream by `read_options_finalize`), so a real
                    // `compile_client` run never reaches this fold with a `tag`
                    // attribute. If it ever does (a direct resolver call bypassing the
                    // gate), produce the SAME official rejection rather than silently
                    // folding the tag away — a fail-open silent `Ok` would drop the
                    // deprecated-tag error.
                    return Err(UnsupportedSvelteRuntimeSurface::OfficialReject {
                        rejection: OfficialRejection::with_code(
                            CoreOfficialValidationRule::OptionsInvalid,
                            "svelte_options_deprecated_tag",
                        ),
                        span: el.open_span,
                    });
                }
                // `runes` flows through the existing mode-inference plumbing
                // (`forced_runes_option` + `opts.runes`); `css` / `customElement`
                // are owned by their own resolvers. None are folded here.
                _ => {}
            }
        }
    }

    // The compile-option fail-closed rejects (CompileProfile origin) — EXPLICIT
    // presence rejects, including a `false` / default-equivalent value.
    //
    // `compatibility.componentApi`: svelte@5.56.3 accepts ONLY {4, 5} — `4` is the
    // Svelte-4 instance-API compat FEATURE this backend refuses; `5` is the current
    // supported API. Any OTHER explicit value (`0`, `6`, …) is an official
    // `options_invalid_value` error, so it ALSO fails closed here (never resolves a
    // module) rather than falling open. Reuse the `CompatibilityComponentApi` refusal
    // to keep the enum closed. `Some(5)` / absent pass unchanged.
    if let Some(api) = opts.compatibility_component_api {
        if api != 5 {
            return Err(compile_profile_reject(
                UnsupportedSvelteCompileOption::CompatibilityComponentApi,
            ));
        }
    }
    if opts.hmr.is_some() {
        return Err(compile_profile_reject(UnsupportedSvelteCompileOption::Hmr));
    }
    if opts.accessors.is_some() {
        return Err(compile_profile_reject(
            UnsupportedSvelteCompileOption::Accessors,
        ));
    }
    if opts.immutable.is_some() {
        return Err(compile_profile_reject(
            UnsupportedSvelteCompileOption::Immutable,
        ));
    }

    // Fold the namespace (inline wins over the compile option, then the default) and
    // REFUSE a non-`html` selection: this backend emits html-namespace roots only, so
    // svg / mathml is a fail-closed feature refusal (the winning namespace + its
    // origin — Inline when the inline value won, else the compile profile).
    let namespace = inline_namespace
        .or(opts.namespace)
        .unwrap_or(SvelteNamespace::Html);
    if namespace != SvelteNamespace::Html {
        let (origin, span) = if inline_namespace.is_some() {
            (CompileOptionOrigin::Inline, options_span)
        } else {
            (CompileOptionOrigin::CompileProfile, None)
        };
        return Err(UnsupportedSvelteRuntimeSurface::NamespaceUnsupported {
            namespace,
            origin,
            span,
        });
    }

    // Fold: inline wins over the compile option, then the default.
    let fragments = opts.fragments.unwrap_or(SvelteFragments::Html);
    let preserve_whitespace = inline_preserve_ws
        .or(opts.preserve_whitespace)
        .unwrap_or(false);
    let preserve_comments = opts.preserve_comments.unwrap_or(false);
    let disclose_version = opts.disclose_version.unwrap_or(true);

    Ok(ResolvedSvelteCompileOptions {
        fragments,
        preserve_whitespace,
        preserve_comments,
        disclose_version,
    })
}

/// The plain-attribute value of a `<svelte:options>` attribute, or `None` for a
/// spread / directive / `{@attach}` (none is a folded axis).
fn plain_attr_value(attr: &SvelteAttribute) -> Option<&Option<SvelteAttributeValue>> {
    match &attr.kind {
        SvelteAttributeKind::Plain { value, .. } => Some(value),
        SvelteAttributeKind::Spread(_)
        | SvelteAttributeKind::Directive(_)
        | SvelteAttributeKind::Attach { .. } => None,
    }
}

/// Map the shared parser namespace token to the typed [`SvelteNamespace`].
///
/// The token comes from `options_namespace_value` — the shared span-based parser
/// value-authority — which admits ONLY the three valid namespace tokens (the SAME
/// authority the official-reject validity gate uses, so the accept-set and the
/// resolved value cannot diverge). Any other token is unreachable by construction.
fn map_namespace(token: &str) -> SvelteNamespace {
    match token {
        "html" => SvelteNamespace::Html,
        "svg" => SvelteNamespace::Svg,
        "mathml" => SvelteNamespace::Mathml,
        other => unreachable!("map_namespace received a non-namespace token: {other:?}"),
    }
}

/// A compile-profile fail-closed refusal (no source span).
fn compile_profile_reject(
    option: UnsupportedSvelteCompileOption,
) -> UnsupportedSvelteRuntimeSurface {
    UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported {
        option,
        origin: CompileOptionOrigin::CompileProfile,
        span: None,
    }
}

#[cfg(test)]
#[path = "compile_options_tests.rs"]
mod tests;
