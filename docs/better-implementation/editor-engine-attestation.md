# Editor-engine attestation: deferred better-design backlog

Status: **design reference (deferred)** — not a committed plan step. These are the improvements an independent
architecture consult identified around the editor-engine observation and the `auto` provider decision, and
which were explicitly ruled **non-blocking**. This document records them so they are not lost and not silently
re-litigated; it does not authorize or schedule the work.

Everything the shipped implementation *does* is documented at its source
(`crates/verter_lsp/src/provider_decision.rs`, `packages/vue-vscode/src/editorEngine.ts`). This file is only
the deferred remainder.

---

## 1. What shipped, in one paragraph

The `auto` provider decision is a **capability join** over `Tsserver < Tsgo`. The workspace engine the server
discovers for itself is a verified **floor**; the editor's reported engine is **upward-only evidence** that may
raise that floor but never lower it; the bundled engine is the capability bottom, eligible only when no higher
floor exists.

The client attests tsgo only under the conjunction of the **two shipped gates**, which read the same two
settings by **different rules**: the built-in TypeScript extension's **stand-down** gate (whether classic
tsserver steps aside — unified `js/ts` primary, legacy `typescript` a whole-section fallback) and the Native
Preview extension's **start** gate (whether tsgo actually runs — the two sections ranked by
`ConfigurationTarget` specificity), plus the Native Preview extension being **present** at all, since tsgo ships
inside it. Mirroring either gate alone fabricates tsgo on real configurations where the two disagree. It
**omits** the observation whenever it cannot attest, and never reports `tsserver`.

Attestation alone is not enough to serve the user: tsgo ships **inside** the Native Preview extension and is not
in a normal project's `node_modules`, so the client also resolves that extension's own `lib/` and hands the
engine path to the server through its existing `VERTER_TSGO_BIN` discovery tier. Raising the floor to tsgo
without supplying a runnable tsgo is what turns the bug into a worse bug — a typed `Unavailable` and no type
provider at all.

---

## 2. Deferred: runtime attestation via the Native Preview extension's exported API

The Native Preview extension exports `{ onLanguageServerInitialized: Event<void>; initializeAPIConnection(pipe?):
Promise<string> }` via `await ext.activate()`. That attests tsgo **is actually running**, which is strictly
stronger than what we attest today: that both shipped gates say it *should* run and the engine is installed —
not that the process actually came up.

What we attest today is the conjunction described in §1: the extension is **present**, the built-in **stands
down**, and Native Preview **starts**. Each conjunct is merely NECESSARY. The setting is Settings-Sync'd and the
extension auto-writes it on install without removing it on uninstall, so a synced profile with no extension
carries `useTsgo: true` while the editor runs classic tsserver; an installed engine the user has not switched on
is not an engine the editor is running; and — the reason the two gates must BOTH be consulted — the built-in can
stand tsserver down on a configuration where Native Preview's scope ranking still refuses to start, leaving the
editor with no engine at all.

Deferred because: the exported API carries no third-party compatibility promise, and it resolves
**asynchronously, after** our own `initialize`, so consuming it would require a re-decision channel that does not
exist today. It is an upgrade in fidelity — it would close the residual gap where both gates agree but the tsgo
process fails to start — not a fabrication risk.

## 3. Deferred: per-project routing for mixed-engine multi-root workspaces

The decision is per **server process**, so a multi-root workspace whose roots want different engines gets one
engine. This is a pre-existing single-provider limitation, not a defect introduced by the observation channel
(the editor's engine is an editor-process-wide fact, so the observation itself is not made stale by multi-root).

Deferred because: it is a scope change to the provider lifecycle (one provider per project rather than per
process), not a fix to the decision.

## 4. Deferred: distinguishing "did not look" from "looked, could not tell"

The client omits the observation both when it did not attempt attestation and when it attempted and could not
determine the engine. Those two states lead the decision to the **identical** action — fall through to the
discovery tier — so the distinction changes no outcome.

If the distinction is ever wanted it is a **telemetry** concern and belongs in the audit envelope (which is
additive by contract), **never** as an `unknown` engine variant: a closed-enum variant that changes no decision
buys a permanent exhaustive-match cost for nothing.

## 5. Deferred: the capability order inverts where a composite tsconfig is present

The join ranks `Tsgo` above `Tsserver` unconditionally. But `has_composite` exists in the model precisely because
a solution-style project is one place tsserver has historically been *preferred* — so for that one input the real
capability order is arguably the reverse of the lattice's.

The consequence is deliberate and consistent: a discovered-tsgo workspace already beats a composite tsconfig
today (the pre-existing tsgo veto "wins even over a composite tsconfig"), so an *attested-tsgo editor* beating it
is the same rule applied to the same evidence class. A user whose editor runs tsgo is already living with that
engine's referenced-config behaviour in their `.ts` files; matching it in `.vue` is coherent rather than
surprising.

Recorded so the ordering is a knowing choice rather than an accident. If composite handling ever needs to
*outrank* an engine upgrade, that is a second axis (a per-capability lattice rather than a single engine order),
not a tweak to this one.

## 6. Deferred: bound the verified floor to the workspace root

`find_tsserver_workspace_only` walks up to ten ancestor directories, mirroring `find_tsserver`'s tier 1 and
`active_typescript_is_tsgo`. Its result therefore carries **ancestor** provenance, not strictly *workspace*
provenance: a TypeScript installed above the workspace root (a parent monorepo, or a stray home-directory
install) can become the VERIFIED FLOOR — which now carries real semantic weight, since the floor suppresses the
tsgo refusal and selects the fallback engine.

The walk is the established convention in this codebase and monorepo members legitimately depend on it to find a
hoisted TypeScript, so narrowing it is a behavioural change well beyond this fix. But CLAUDE.md's own principle
("owned resolution is bounded by `workspace_root`") points the other way, and the honest position is that the
function guarantees only one thing today: it never reaches the `--tsdk` bundle or a global install. That is the
property the fix depends on, and it is the property its documentation now claims — no more.

## 7. Deferred: warning on an explicit downgrade

A user who explicitly configures `verter.typeProvider: "tsserver"` on a discovered-tsgo workspace is **honoured**
— an explicit setting is a command, not an assertion about the world, and the capability lattice governs
*evidence*, not *policy*. Refusing it could strand a user with no working provider at all, which is a worse
outcome than the one the lattice exists to prevent.

A surfaced warning on that path (you have asked for a lower engine than your workspace's) would be a genuine
usability improvement. It is a product decision, not a correctness fix.

## 8. Deferred: report the provider-decision reason for every kind, not only `none`

`decide` computes a reason on **every** arm, and `create_type_provider` then discards it on all four success
paths (`main.rs` returns `None` alongside `TypeProviderKind::Tsgo` / `Tsserver`, using the string only for a
`tracing` line the user never sees). Two further gates finish the job: the carrier field is named
`type_provider_none_reason`, and `server/lifecycle.rs` explicitly nulls it unless the kind is `None`:

```rust
let reason = if matches!(server.type_provider_kind, TypeProviderKind::None) {
    server.type_provider_none_reason.clone()
} else {
    None
};
```

So Verter tells the user **which** engine it selected and never **why** — and "why did it pick the broken
TypeScript 6?" is precisely the question the reported bug left the user unable to answer. The engine is now
chosen by a capability JOIN over several discovered facts, which makes the selection *more* worth explaining,
not less.

The change is small and REMOVES a special case rather than adding one: populate the existing optional `reason`
on the four success arms, rename `type_provider_none_reason` -> `type_provider_reason`, and delete the
`lifecycle.rs` gate. It needs **no** client change (`extension.ts` already renders `params.reason` for any kind)
and **no** wire-shape change (the field exists and is `skip_serializing_if = "Option::is_none"`). The status-bar
tooltip would then read, e.g., *"the editor reports a tsgo TypeScript engine (raising over the discovered
tsserver selection)"*.

It is deferred here only because it is an observability improvement rather than a correctness fix: the decision
is already correct without it, and this change's scope is the decision, not its reporting. Note the knock-on for
tests — `parseStartupTiming` in the e2e helpers likewise exposes `typeProviderReason` only when the kind is
`none`, so the e2e suite cannot currently assert the server's own explanation of a successful selection. It pins
the discovered floor directly instead (`editor-tsgo-observation.test.ts`); the reason assertion would be the
stronger guard, because it asserts what the server ACTUALLY DID rather than what the environment affords.
