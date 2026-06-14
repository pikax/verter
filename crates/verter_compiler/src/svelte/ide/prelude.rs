//! The Svelte IDE-projection ambient prelude.
//!
//! Every projected `.svelte.tsx` opens with one UNMAPPED prelude inserted at
//! output offset 0 (D-u / D-ad / D-ae). It carries three things, none of which
//! shift a mapped position (the prelude is pure insertion — every original
//! script/template byte keeps its source offset):
//!
//! 1. the per-file `@jsxImportSource @verter/svelte-jsx` pragma — overriding
//!    the provider's project-level `jsxImportSource: "vue"` for THIS file only
//!    (D-ae(a)), even under `jsx: "preserve"`;
//! 2. the COMPLETE audited Svelte 5 rune surface as ambient `declare`s
//!    (`$props`/`$bindable`/`$state`/`$derived`/`$effect`/`$inspect`/`$host`,
//!    every namespace member, and `import type { Snippet } from "svelte"`) —
//!    rune CALL SITES stay verbatim, the prelude only TYPES them (D-u);
//! 3. the projection checkers/declarators (D-ae(c)): `__verter_attach` (the
//!    `{@attach}` target), the `__verter_snippet` brand declarator, and the
//!    `__verter_void` value checker (out-of-scope expressions route through it).
//!    `class` clsx forms (5.16) are typed by `SvelteHTMLElements`'
//!    `class?: ClassValue` in the intrinsic table — no separate class checker.
//!
//! The rune list is COMPLETE so no fixture using a namespace member fails the
//! clean-type-check gate spuriously. The declarations are ambient (`declare`)
//! so they introduce no runtime value and never collide with a user import.

/// The per-file pragma line. Opens the prelude; overrides the project-level
/// `jsxImportSource` for this file only.
pub const PRAGMA_LINE: &str = "/** @jsxImportSource @verter/svelte-jsx */\n";

/// Render the complete UNMAPPED prelude text.
///
/// The result is a single block of inserted text. It is deterministic and
/// self-contained (every referenced type is imported or declared here), so a
/// fixture's only un-checked names are the user's own script/template
/// symbols.
#[must_use]
pub fn render_prelude() -> String {
    // One static block — the rune surface is fixed (the audited Svelte 5.56.x
    // surface), so a const string is exact and allocation-free to assemble.
    let mut out = String::with_capacity(PRAGMA_LINE.len() + RUNE_AND_CHECKER_PRELUDE.len());
    out.push_str(PRAGMA_LINE);
    out.push_str(RUNE_AND_CHECKER_PRELUDE);
    out
}

/// The ambient rune surface + the three checkers/declarators.
///
/// Ambient (`declare`) so it introduces no runtime binding. `$props` etc. are
/// declared as functions with their namespace members attached via a merged
/// `declare namespace`. The Svelte 5 surface is COMPLETE per the D-u/D-ad
/// audit (5.56.x): `$state.raw`/`.snapshot`/`.eager`, `$derived.by`,
/// `$effect.pre`/`.tracking`/`.root`/`.pending`, `$inspect(...).with`/`.trace`,
/// `$props.id`, `$host`.
const RUNE_AND_CHECKER_PRELUDE: &str = r#"import type { Snippet } from "svelte";
import type { Attachment } from "svelte/attachments";
// --- Svelte 5 runes (ambient; call sites stay verbatim) ---
declare function $props<T = Record<string, unknown>>(): T;
declare namespace $props {
  function id(): string;
}
declare function $bindable<T = never>(fallback?: T): T;
declare function $state<T>(initial: T): T;
declare function $state<T>(): T | undefined;
declare namespace $state {
  function raw<T>(initial: T): T;
  function raw<T>(): T | undefined;
  function snapshot<T>(state: T): T;
  function eager<T>(initial: T): T;
}
declare function $derived<T>(expression: T): T;
declare namespace $derived {
  function by<T>(fn: () => T): T;
}
declare function $effect(fn: () => void | (() => void)): void;
declare namespace $effect {
  function pre(fn: () => void | (() => void)): void;
  function tracking(): boolean;
  function root(fn: () => void | (() => void)): () => void;
  function pending(): boolean;
}
declare function $inspect<T extends unknown[]>(...values: T): { with: (fn: (type: "init" | "update", ...values: T) => void) => void };
declare namespace $inspect {
  function trace(name?: string): void;
}
declare function $host<El extends HTMLElement = HTMLElement>(): El;
// --- Verter projection checkers/declarators (D-ae(c)) ---
declare function __verter_attach<E extends EventTarget>(attachment: Attachment<E>): void;
declare function __verter_snippet<Params extends unknown[]>(render: (...args: Params) => unknown): Snippet<Params>;
declare function __verter_void(...values: unknown[]): void;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prelude_opens_with_the_jsx_import_source_pragma() {
        let prelude = render_prelude();
        assert!(prelude.starts_with("/** @jsxImportSource @verter/svelte-jsx */"));
    }

    #[test]
    fn prelude_declares_the_complete_rune_surface() {
        let p = render_prelude();
        for needle in [
            "declare function $props",
            "function id()",
            "declare function $bindable",
            "declare function $state",
            "function raw<",
            "function snapshot<",
            "function eager<",
            "declare function $derived",
            "function by<",
            "declare function $effect",
            "function pre(",
            "function tracking()",
            "function root(",
            "function pending()",
            "declare function $inspect",
            "function trace(",
            "declare function $host",
        ] {
            assert!(p.contains(needle), "prelude missing rune member: {needle}");
        }
    }

    #[test]
    fn prelude_declares_the_three_checkers_and_imports_snippet() {
        let p = render_prelude();
        assert!(p.contains("import type { Snippet } from \"svelte\""));
        assert!(p.contains("declare function __verter_attach"));
        assert!(p.contains("declare function __verter_snippet"));
        assert!(p.contains("declare function __verter_void"));
        // The `class` clsx form (5.16) is typed by `SvelteHTMLElements`'
        // `class?: ClassValue` in the intrinsic table — no dead
        // `__verter_class` declarator (NIT-1).
        assert!(!p.contains("__verter_class"), "dead declarator removed");
    }
}
