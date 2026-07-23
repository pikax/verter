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
use crate::code_transform::CodeTransform;
use verter_span::Span;

pub(super) fn append_svelte_public_facade<'a>(
    ct: &mut CodeTransform<'a>,
    at: u32,
    props: Option<(&str, Span)>,
    dialect: SvelteIdeDialect,
) {
    let (props_ty, source_span) = props
        .map(|(text, span)| (text, Some(span)))
        .unwrap_or(("Record<string, unknown>", None));
    let (prefix, suffix) = match dialect {
        SvelteIdeDialect::TypeScript => (
            "\ndeclare const __VerterPublicComponent: import(\"svelte\").Component<",
            ", {}, \"\">;\nexport default __VerterPublicComponent;\n",
        ),
        SvelteIdeDialect::JavaScript => (
            "\nconst __VerterPublicComponent = /** @type {import(\"svelte\").Component<",
            ", {}, \"\">} */ (/** @type {unknown} */ (null));\nexport default __VerterPublicComponent;\n",
        ),
    };
    let props_ty = ct.alloc_str(props_ty);
    let mut fragments = Vec::with_capacity(props_ty.lines().count() + 2);
    fragments.push((at, None, prefix));
    if let Some(span) = source_span {
        // An InsertedMapped chunk owns one source-map token. Source-map state
        // does not carry across a generated newline, so a multiline authored
        // annotation must start a mapped chunk on every line. The copied bytes
        // are identical to the source span; advancing by each inclusive line's
        // byte length therefore preserves exact generated/source coordinates.
        let mut source_offset = 0u32;
        for line in props_ty.split_inclusive('\n') {
            fragments.push((at, Some((span.start + source_offset, 0)), line));
            source_offset += line.len() as u32;
        }
    } else {
        fragments.push((at, None, props_ty));
    }
    fragments.push((at, None, suffix));
    ct.batch_prepend_left_with_source_map(&fragments);
}
