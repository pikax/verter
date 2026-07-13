//! The CONTROL-FLOW block + block-local declaration / `{@debug}` carriers of
//! the narrow client plan (`{#if}`/`{#each}`/`{#await}`/`{#key}` heads, the
//! `{@const}` / declaration-tag declarations, the debug entries) — extracted
//! from `client_plan_types` under the file-size guard. Every head expression
//! is a PREPARED authored value (see
//! [`super::client_legacy_value::PreparedTemplateValue`]); the emitters
//! serialize the carriers and never re-derive a wrap.

use super::client_legacy_value::PreparedTemplateValue;
use super::ir::TemplateScopeId;

/// A control-flow block with its head expressions rewritten + child-region scope ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientBlock {
    /// `{#if}` chain — branches in source order; the trailing `test: None` branch is
    /// the `{:else}`.
    If {
        /// The if/else-if/else branches, in source order.
        branches: Vec<ClientIfBranch>,
    },
    /// `{#each}` — keyed/unkeyed, optional index, optional `{:else}`.
    Each(ClientEach),
    /// `{#await}` — pending/then/catch.
    Await(ClientAwait),
    /// `{#key expr}` — `$.key(node, () => expr, ($$anchor) => { … })`.
    Key {
        /// The PREPARED key expression (the emitter supplies the thunk).
        expr: PreparedTemplateValue,
        /// The body region.
        body: TemplateScopeId,
    },
}

/// One branch of an `{#if}` chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientIfBranch {
    /// The PREPARED branch test, or `None` for the `{:else}` branch.
    pub(super) test: Option<PreparedIfCondition>,
    /// The branch body region.
    pub(super) body: TemplateScopeId,
}

/// A PREPARED `{#if}` / `{:else if}` condition: the prepared test value plus
/// the official call-bearing topology — a `has_call` test hoists an outer
/// `$.derived(() => <prepared>)` (UNCONDITIONAL on mode — never
/// `$.derived_safe_equal`) and the branch tests `$.get(<id>)`
/// (official `IfBlock.js`). The emitter serializes the prepared condition and
/// prelude; it never inspects legacy mode or reconstructs a wrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreparedIfCondition {
    /// The prepared test value.
    pub(super) value: PreparedTemplateValue,
    /// The hoisted `$.derived` read for a call-bearing test; `None` inline.
    pub(super) call_derived: Option<PreparedDerivedRead>,
}

/// The hoisted `$.derived` of a call-bearing `{#if}` test — the emitter
/// allocates the collision-free `d` name and emits
/// `var <d> = $.derived(() => <thunk_body>);` then reads `$.get(<d>)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreparedDerivedRead {
    /// The derived thunk BODY (the prepared test's memo/arrow form).
    pub(super) thunk_body: String,
}

/// A projected `{#each}` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientEach {
    /// The official EACH flags bitmask (`EACH_ITEM_REACTIVE` | `EACH_INDEX_REACTIVE` |
    /// `EACH_IS_CONTROLLED` | `EACH_ITEM_IMMUTABLE`).
    pub(super) flags: u8,
    /// The PREPARED source expression (the emitter supplies the thunk).
    pub(super) source: PreparedTemplateValue,
    /// The KEY callback for a keyed each (`(item) => key`), or `None` for an unkeyed
    /// each (emitted as the `$.index` literal).
    pub(super) key: Option<ClientEachKey>,
    /// The item binding param name (`None` for the no-item `{#each {length}}` form).
    pub(super) item_param: Option<String>,
    /// The index binding param name, emitted ONLY when [`ClientEach::emit_index`] is set.
    pub(super) index_param: Option<String>,
    /// Whether the index render param is emitted (the official `uses_index` rule: the
    /// index is read, OR the item is reassigned / mutated).
    pub(super) emit_index: bool,
    /// The body region.
    pub(super) body: TemplateScopeId,
    /// The `{:else}` fallback region.
    pub(super) else_body: Option<TemplateScopeId>,
}

/// The key callback of a keyed `{#each}` — emitted in its OWN callback scope (the key
/// expression is PLAIN, never body-signal-rewritten).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientEachKey {
    /// The key-callback params (`(item)` or `(item, index)` when the key reads the index).
    pub(super) params: Vec<String>,
    /// The PREPARED key expression (raw-policy — official keyed-each keys are
    /// raw), rewritten in the KEY scope (NOT body-signal-rewritten).
    pub(super) expr: PreparedTemplateValue,
}

/// A projected `{#await}` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientAwait {
    /// The PREPARED promise expression (the emitter supplies the thunk).
    pub(super) promise: PreparedTemplateValue,
    /// The pending body region (`None` → the `null` argument slot).
    pub(super) pending: Option<TemplateScopeId>,
    /// The `{:then v}` value param name.
    pub(super) then_param: Option<String>,
    /// The `{:then}` body region.
    pub(super) then_body: Option<TemplateScopeId>,
    /// The `{:catch e}` error param name.
    pub(super) catch_param: Option<String>,
    /// The `{:catch}` body region.
    pub(super) catch_body: Option<TemplateScopeId>,
}

/// One block-local declaration (a `{@const}` derived memo, a `{const}/{let}` inert
/// declarator, or a rune-carrying `{let x = $state(…)}` declarator).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientDeclaration {
    /// `{@const x = INIT}` → `const x = <helper>(() => INIT);` with the
    /// mode-aware helper (official `utils.js` `create_derived`: `$.derived`
    /// in runes mode, `$.derived_safe_equal` in EVERY non-runes mode) over the
    /// PREPARED initializer (legacy wrap applies only in definite legacy).
    Derived {
        /// The declared name.
        name: String,
        /// The PREPARED initializer.
        init: PreparedTemplateValue,
        /// The mode-selected derived helper.
        helper: DerivedHelper,
    },
    /// `{const x = INIT}` / `{let x = INIT}` / `{let x}` inert declarator → a plain
    /// block-local `const`/`let` (NO `$.derived`, NO `$.get`); the initializer is
    /// signal-rewritten but the binding itself is inert.
    Inert {
        /// The declaration keyword.
        keyword: ClientDeclKeyword,
        /// The declared name.
        name: String,
        /// The rewritten initializer, or `None` for a bare `let x;`.
        init: Option<String>,
    },
    /// A rune-carrying `{let x = $state(…)}` / `{let x = $derived(…)}` declarator,
    /// classified through the instance-script rune/state pipeline → the already-lowered
    /// declaration statement (`let x = $.state(…)` / `let x = $.derived(…)`).
    Rune {
        /// The fully-lowered declaration statement (without trailing `;`).
        code: String,
    },
}

/// The mode-aware `{@const}` derived helper (official `create_derived`):
/// `$.derived` ONLY in runes mode; `$.derived_safe_equal` in every non-runes
/// mode (definite legacy AND maybe-runes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DerivedHelper {
    /// `$.derived` (runes mode).
    Derived,
    /// `$.derived_safe_equal` (every non-runes mode).
    DerivedSafeEqual,
}

impl DerivedHelper {
    /// The emitted helper name.
    pub(super) fn name(self) -> &'static str {
        match self {
            DerivedHelper::Derived => "$.derived",
            DerivedHelper::DerivedSafeEqual => "$.derived_safe_equal",
        }
    }
}

/// The declaration keyword of an inert `{const}/{let}` declarator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClientDeclKeyword {
    /// `const`.
    Const,
    /// `let`.
    Let,
}

/// One `{ key: $.snapshot(arg) }` entry of a `{@debug}` effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientDebugEntry {
    /// The object key (the debug identifier name).
    pub(super) key: String,
    /// The rewritten `$.snapshot(<expr>)` argument expression.
    pub(super) snapshot_arg: String,
}
