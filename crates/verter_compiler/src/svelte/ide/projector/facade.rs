//! The Svelte IDE-carrier PUBLIC-FACADE default export.
//!
//! Extracted from the projector `mod.rs` to keep that file under the size guard
//! (`no_oversize_files`). The facade is the component's public type composed on
//! the IDE (self-diagnostics) carrier `Comp.svelte.tsx`. The bare-import-probe
//! identity a consumer's `import Comp from "./Comp.svelte"` resolves to is the
//! DECLARATION carrier (`Comp.d.svelte.ts`, the path tsgo's basename-append
//! probe reaches first), §2.2/§2.9 — not this IDE carrier.

/// Synthesise the Svelte component's PUBLIC-FACADE default export for the IDE
/// carrier (the self-diagnostics surface). Mirrors the MINIMAL facade
/// SHAPE the higher-layer API projector emits on the `.svelte.verter.ts` API
/// carrier — a native callable Svelte 5 `Component<Props, Exports, Bindings>` —
/// so the IDE/self-diagnostics surface carries the real
/// public component type for the component's OWN editing (the two crates cannot
/// share code; `verter_compiler` is the lower crate). An in-project consumer's
/// bare `import Comp from "./Comp.svelte"` resolves to the `.d.svelte.ts`
/// DECLARATION carrier (§2.2/§2.9), NOT this IDE carrier.
///
/// `props_type` is the `$props()` annotation, derived SYNTACTICALLY (LOCAL — no
/// resolver); `None` ⇒ a permissive `Record<string, unknown>`. Template
/// internals stay LOCAL. The `__VerterPublic*`
/// prefix avoids collision with user bindings or the `__VerterSelf*` contract.
use super::super::SvelteIdeDialect;

pub(super) fn svelte_public_facade(props_type: Option<&str>, dialect: SvelteIdeDialect) -> String {
    let props_ty = props_type.unwrap_or("Record<string, unknown>");
    match dialect {
        SvelteIdeDialect::TypeScript => format!(
            "\ntype __VerterPublicProps = {props_ty};\n\
             declare const __VerterPublicComponent: import(\"svelte\").Component<__VerterPublicProps, {{}}, \"\">;\n\
             export default __VerterPublicComponent;\n",
        ),
        SvelteIdeDialect::JavaScript => format!(
            "\n/** @typedef {{{props_ty}}} __VerterPublicProps */\n\
             const __VerterPublicComponent = /** @type {{import(\"svelte\").Component<__VerterPublicProps, {{}}, \"\">}} */ (/** @type {{unknown}} */ (null));\n\
             export default __VerterPublicComponent;\n",
        ),
    }
}
