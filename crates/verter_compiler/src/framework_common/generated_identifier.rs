//! The compiler-generated identifier namespace, owned once by the emitter
//! crate and shared with every consumer that has to recognise it.
//!
//! Verter's IDE projections declare helpers, brands, pragma bindings and
//! locals that exist only so TypeScript can type-check the generated carrier.
//! None of them is source the user can name, so no consumer surface (LSP
//! completion above all) may present them.
//!
//! The namespace is a RULE, not a list: every generated identifier is two or
//! more leading underscores followed by the `verter` root, in one of the two
//! shapes the emitter actually mints —
//!
//! - a CAPITALISED root, which needs no separator because the capital is the
//!   separator: `___VERTER___TemplateBindingFN` (Vue IDE codegen),
//!   `__VERTER__TS__T` (its synthesised type parameters), `__VerterSnippet`
//!   and `__Verter_EmitFn` (the Svelte IDE prelude brands);
//! - a lowercase root followed by `_`: `__verter_bind_rw` (the Svelte IDE
//!   prelude helpers), `__verter_vue_jsx_<hash>` and
//!   `__verter_svelte_jsx_<hash>` (the provider JSX pragma bindings).
//!
//! The lowercase arm carries its `_` by construction, not by convention: every
//! such binding is composed on top of [`GENERATED_IDENTIFIER_PREFIX`]. So the
//! root must END at the separator. A bare case-insensitive prefix would also
//! swallow `__verterStore` — an ordinary authored binding, which every consumer
//! then silently removes, leaving the user unable to complete their own
//! variable.
//!
//! A list is what let the Svelte prelude's two-underscore helpers through a
//! filter built for Vue's three-underscore spelling; the rule cannot have that
//! gap. Note the residue the rule keeps: an authored `__VerterStore` is
//! byte-identical in shape to the minted brands, so it stays namespace-owned.
//! That collision is not separable by any rule, and capitalised-after-leading-
//! underscores is not how a binding gets named by hand.
//!
//! Two closed families sit beside the underscore rule because they predate it
//! and are spelled differently: the string-exported type helpers (`$V_…`) and
//! the fixed set of script-projection locals (`__props`, `__emit`, …).

/// The prefix every programmatically minted generated binding is built from.
///
/// Both provider JSX pragma binding minters compose their identifier on top of
/// this, so a minted binding is inside the namespace [`is_generated_identifier`]
/// recognises by construction rather than by coincidence.
pub const GENERATED_IDENTIFIER_PREFIX: &str = "__verter_";

/// The string-exported type-helper prefix.
const TYPE_HELPER_PREFIX: &str = "$V_";

/// The closed set of two-underscore script-projection locals.
const SCRIPT_PROJECTION_LOCALS: [&str; 5] = ["props", "emit", "slots", "expose", "returned"];

/// The generated-namespace root, matched case-insensitively after the leading
/// underscores (`verter`, `Verter`, `VERTER`) and required to end at a
/// separator — see the module docs for why the lowercase spelling needs one.
const NAMESPACE_ROOT: &str = "verter";

/// Whether `label` names a compiler-generated carrier identifier.
///
/// This is the single authority every consumer filters on. It is deliberately
/// conservative at both boundaries: one leading underscore is not enough
/// (`_verter` is authorable source), the root must match exactly, so
/// `__vertex` and `__vertical` stay user identifiers, and the root must END
/// at a separator, so the authored `__verterStore` stays a user identifier too.
///
/// `label` is untrusted identifier text — a completion request carries whatever
/// the user typed, and non-ASCII identifiers are legal JS/TS. The root is
/// therefore compared over BYTES: a `str` slice at a fixed byte offset would
/// panic the request thread the moment a multibyte character straddled it.
pub fn is_generated_identifier(label: &str) -> bool {
    if label.starts_with(TYPE_HELPER_PREFIX) {
        return true;
    }
    let rest = label.trim_start_matches('_');
    let underscores = label.len() - rest.len();
    if underscores < 2 {
        return false;
    }
    let root = NAMESPACE_ROOT.as_bytes();
    let rest_bytes = rest.as_bytes();
    if rest_bytes.len() >= root.len()
        && rest_bytes[..root.len()].eq_ignore_ascii_case(root)
        // The root ends here: capitalised (the capital is the separator), at
        // the end of the label, or at the `_` every minted lowercase binding
        // carries from `GENERATED_IDENTIFIER_PREFIX`.
        && (rest_bytes[0].is_ascii_uppercase()
            || rest_bytes.len() == root.len()
            || rest_bytes[root.len()] == b'_')
    {
        return true;
    }
    underscores == 2 && SCRIPT_PROJECTION_LOCALS.contains(&rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_every_emitted_generated_shape() {
        for label in [
            // Vue IDE codegen — three underscores, uppercase root.
            "___VERTER___TemplateBindingFN",
            "___VERTER___EventKey",
            "___VERTER___instance",
            "___VERTER___ctx",
            "___VERTER___props",
            "___VERTER___slots",
            "___VERTER___emits",
            "___VERTER___Component",
            "___VERTER___Prettify",
            "___VERTER___defineProps_Type",
            "___VERTER___strictRenderSlot",
            "___VERTER___normalizeClass",
            // Vue IDE codegen synthesised type parameters.
            "__VERTER__TS__T",
            "__VERTER__TS__K",
            // Svelte IDE prelude — two underscores, lowercase root.
            "__verter_bind_rw",
            "__verter_snippet",
            "__verter_store_get",
            "__verter_render",
            "__verter_bind_group",
            "__verter_store_set",
            "__verter_await_expr",
            "__verter_attach",
            "__verter_transition",
            "__verter_component_render",
            // Svelte IDE prelude type brands — two underscores, PascalCase root.
            "__VerterSnippet",
            "__VerterPublicComponent",
            "__VerterAutomaticJSX",
            "__VerterAttrsType",
            "__VerterEventHandler",
            "__VerterHostEl",
            "__VerterComponentProps",
            "__Verter_EmitFn",
            "__Verter_UnionToIntersection",
            // The deliberate residue: an authored name in the capitalised brand
            // shape is not separable from a minted one, so it stays owned.
            "__VerterStore",
            // Provider JSX pragma bindings.
            "__verter_vue_jsx_a26e561345c18485da67ce23",
            "__verter_svelte_jsx_c8748e7dc3f332cef005fd50",
            // String-exported type helpers.
            "$V_Props",
            // Script-projection locals.
            "__props",
            "__emit",
            "__slots",
            "__expose",
            "__returned",
        ] {
            assert!(
                is_generated_identifier(label),
                "{label} is a generated carrier identifier"
            );
        }
    }

    /// The emitter never mints a lowercase root without its `_` delimiter — it
    /// composes every such binding on top of [`GENERATED_IDENTIFIER_PREFIX`].
    /// An author who names a binding `__verterStore` is therefore outside every
    /// generated family, and hiding it would silently delete a user's own
    /// variable from completion.
    #[test]
    fn authored_names_below_the_lowercase_namespace_stay_visible() {
        for label in [
            "__verterStore",
            "__verterX",
            "__verterstore",
            "___verterHelper",
            // Authored identifiers that already exist in this repository's own
            // fixtures.
            "__verterDebugSession",
            "__verterMissingPointerMember",
        ] {
            assert!(
                !is_generated_identifier(label),
                "{label} is authorable source, not a generated identifier"
            );
        }
    }

    /// Non-ASCII identifiers are legal JS/TS and reach this predicate straight
    /// off a completion request, so the namespace check must never index into
    /// the middle of a multibyte character.
    #[test]
    fn non_ascii_identifiers_are_classified_without_panicking() {
        for label in [
            // `量` straddles byte 6, the end of the namespace root.
            "__v变量x",
            "__ab变量",
            // A four-byte character straddling the same boundary.
            "__abc🎉x",
            // Shorter than the root, and exactly as long as it.
            "__é",
            "__日本",
            // Multibyte after an ASCII prefix that nearly spells the root.
            "__ver变ter",
            "__проверка",
        ] {
            assert!(
                !is_generated_identifier(label),
                "{label} is authorable source, not a generated identifier"
            );
        }
    }

    #[test]
    fn leaves_authorable_identifiers_alone() {
        for label in [
            "verter",
            "_verter",
            "_verterHelper",
            "__vertex",
            "__vertical",
            "__vert",
            "__proto__",
            "__dirname",
            "__typename",
            "props",
            "_props",
            "$props",
            "$VueThing",
            "structuredClone",
            "WeakSet",
        ] {
            assert!(
                !is_generated_identifier(label),
                "{label} is authorable source, not a generated identifier"
            );
        }
    }
}
