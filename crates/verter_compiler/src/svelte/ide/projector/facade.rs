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
/// carrier — a constructable component whose instance carries `$props` /
/// `$events` / `$slots` — so the IDE/self-diagnostics surface carries the real
/// public component type for the component's OWN editing (the two crates cannot
/// share code; `verter_compiler` is the lower crate). An in-project consumer's
/// bare `import Comp from "./Comp.svelte"` resolves to the `.d.svelte.ts`
/// DECLARATION carrier (§2.2/§2.9), NOT this IDE carrier.
///
/// `props_type` is the instance `$props()` annotation, derived SYNTACTICALLY
/// (LOCAL — no resolver); `None` ⇒ a permissive `Record<string, unknown>`.
/// `$events` / `$slots` stay permissive shells the consumer re-resolves through
/// the precise API carrier. Template internals stay LOCAL. The `__VerterPublic*`
/// prefix avoids collision with user bindings or the `__VerterSelf*` contract.
pub(super) fn svelte_public_facade(props_type: Option<&str>) -> String {
    let props_ty = props_type.unwrap_or("Record<string, unknown>");
    format!(
        "\ntype __VerterPublicProps = {props_ty};\n\
         interface __VerterPublicInstance {{\n  \
         $props: __VerterPublicProps;\n  \
         $events: Record<string, unknown>;\n  \
         $slots: Record<string, unknown>;\n}}\n\
         declare const __VerterPublicComponent: {{ new (...args: any[]): __VerterPublicInstance }};\n\
         export default __VerterPublicComponent;\n",
    )
}
