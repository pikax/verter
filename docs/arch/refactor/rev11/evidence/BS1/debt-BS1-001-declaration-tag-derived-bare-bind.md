# Tracked debt — BS1-001: declaration-tag `$derived` bare-Identifier bind refuses instead of accepting

Disposition: **DEFER** (per CLAUDE.md "Explicit finding disposition").

## What happened

`landing-record.md` and the doc comment on
`crates/verter_compiler/src/svelte/runtime/client_shapes.rs::is_writable_member_bind_extra_root`
claimed, as "oracle-verified against svelte@5.56.8", that official REJECTS a bare
`bind:value={doubled}` where `doubled` is a `{let doubled = $derived(item)}`
TEMPLATE DECLARATION TAG. Re-run against a fresh `npm install svelte@5.56.8`:

```svelte
<script>let items = $state([{x:'a'}]);</script>
{#each items as item}{let doubled = $derived(item)}<input bind:value={doubled}/>{/each}
```

official ACCEPTS it and emits:

```js
$.bind_value(input, () => $.get(doubled), ($$value) => $.set(doubled, $$value));
```

— the same "overridable derived" bare-reassignment behavior Svelte 5 gives a
genuine `$derived(...)` rune reference. The claim was false; the true behavior
is the opposite of what was documented. Verified separately: the component
`let:` slot-prop case (`<Child let:item><input bind:value={item}/></Child>`)
genuinely DOES reject with `constant_binding` — that half of the doc was
correct and is unchanged.

Consequence: `client_tests.rs::a_member_bind_rooted_at_a_declaration_tag_derived_rune_is_accepted`
carried a negative assertion (`assert_fail_closed` on the bare-Identifier
form) that pins a REAL conformance gap while its comment claimed parity.

## Why this is DEFER, not ADOPT-NOW

Both the declaration-tag rune form and the `let:` slot-prop form lower to the
exact same `BindingRuntimeKind::Derived` binding kind
(`state_prep.rs::classify_block_rune_declarator` /
`lower_component.rs::lower_slot_region`), with no other field on
`BindingInfo` distinguishing them. Official's bare-Identifier behavior
diverges between the two constructs (accepts the rune form, rejects the
slot-prop form), so simply widening `is_writable_bind_root` to admit
`Derived` at the bare-Identifier arm would ALSO wrongly accept the `let:`
slot-prop case, which the oracle confirms must stay refused.

Closing this correctly needs a provenance discriminator threaded from both
minting sites (`state_prep.rs`, `lower_component.rs`) through `BindingInfo`/
`BindingRuntimeKind` to the bare-Identifier classifier, plus a new
bare-Identifier codegen arm emitting `$.set(name, $$value)` for the
rune-provenance case only, plus a check that the widened acceptance does not
regress `client_plan_script.rs`, `client_legacy_value.rs`, or `expr.rs` sites
that already match on `BindingRuntimeKind::Derived`. That is real new
classifier surface, not a reuse of already-implemented machinery — the
top-level instance-script `$derived` form's "overridable derived" behavior
was checked and does NOT already exist in production code either (it never
reaches the bare-Identifier arm at all, refused earlier by
`rune_scan.rs::classify_rune_position`).

## Current (safe) behavior

Verter fails closed on `bind:value={doubled}` for the declaration-tag rune
form, same as the `let:` slot-prop form. Fail-closed is conformant with
neither divergence risk nor a silent miscompile — it is simply stricter than
official for this one construct. `a_member_bind_rooted_at_a_declaration_tag_derived_rune_is_accepted`'s
negative assertion is retained as a KNOWN CONFORMANCE GAP pin (comment
corrected in place), not removed and not weakened.

## Owner

Runes-completion vertical (the same owner the pre-existing top-level
instance-script `$derived` bare-bind divergence is already folded into, per
`rune_scan.rs::classify_rune_position`'s deferral-ledger note) — both
divergences are the same class of gap: Verter has not yet implemented
Svelte 5's "overridable derived" bare-reassignment codegen anywhere.

## Acceptance ID / resolution gate

No pre-existing acceptance ID covers this. Resolution gate, concrete: a
provenance discriminator distinguishes a genuine `$derived(...)` rune
`Derived` binding from a `let:`-slot-prop-synthesized `Derived` binding at
the bare-Identifier classifier, `is_writable_bind_root` admits the rune
provenance, codegen emits `$.set(name, $$value)`, and
`a_member_bind_rooted_at_a_declaration_tag_derived_rune_is_accepted`'s
negative assertion is replaced by a positive assertion of that shape (proven
to fail against the pre-fix tree). The `let:` slot-prop negative assertion
in `a_member_bind_rooted_at_a_derived_binding_is_accepted` must keep
refusing throughout — it is not part of this gap.

## Evidence

Oracle reproduction commands and output captured against a fresh
`npm install svelte@5.56.8` (bare form: `$.set` accepted; `let:` slot-prop
bare form: `constant_binding` refused) — see the corrected doc comment on
`is_writable_member_bind_extra_root` in `client_shapes.rs` for the exact
official-emitted shapes.
