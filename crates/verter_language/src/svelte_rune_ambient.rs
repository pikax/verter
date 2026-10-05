//! The shared Svelte 5 module-rune ambient declarations.
//!
//! The module-valid rune surface (`$state`/`$derived`/`$effect`/`$inspect`
//! and every namespace member, per the audit, 5.56.x) is declared exactly ONCE,
//! here. The Svelte IDE projection renders it into both the component and the
//! standalone rune-module preludes, and the session's per-file
//! eval-environment merge parses the same text so a rune module's exported
//! rune-derived types infer through Verter's own type-resolution engine.
//! Neither consumer carries a second rune-declaration list.
//!
//! This module is plain data: it parses nothing and depends on no parser.

/// The module-VALID rune surface (TS `declare` form).
const MODULE_RUNE_DECLARATIONS: &str = r#"declare function $state<T>(initial: T): T;
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
"#;

/// The module-valid rune ambient declarations (TS `declare` form) WITHOUT a
/// module-local `export {};` marker or any header — the SHARED rune surface a
/// standalone rune module exposes and the component prelude carries verbatim.
///
/// The session's eval-environment merge is already file-scoped, so it parses
/// this text without an `export {};`.
#[must_use]
pub const fn module_rune_ambient_source() -> &'static str {
    MODULE_RUNE_DECLARATIONS
}

/// The version of the rune ambient surface. Bumped whenever the module rune
/// declarations change so a prelude fix invalidates stale inferred exports of a
/// rune module (it enters the rune module's type/eval-env cache key).
pub const RUNE_AMBIENT_PRELUDE_VERSION: u32 = 1;
