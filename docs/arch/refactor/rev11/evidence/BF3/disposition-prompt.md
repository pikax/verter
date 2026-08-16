# HARD EFFICIENCY CONTRACT

ONE turn. Never `cat`/`nl -ba` a file over 80 lines — `sed -n 'A,Bp'` or `rg -n -C 5`. At most
~12 short commands, then WRITE THE RULING. The ruling is the deliverable; no plan.

# Disposition ruling for two findings not covered by the earlier adjudication

You previously adjudicated this block's findings (that ruling is at
`docs/arch/refactor/rev11/evidence/BF3/scope-consult-ruling.md` and a second one you gave
assigned owners `BS0` / `BA0` / `BCSS0` to five defects plus an atomicity violation). Two
further findings surfaced afterwards and have no owner. Rule on them, then confirm or correct
the consolidated table.

Repository: this worktree, read-only. The block is `BF3`
(`docs/arch/refactor/rev11/charters/BF3.md`).

## Finding RT-1 — the batch route compiles every input as Vue

`CompileBatchInput` carries no source-language field, and `compile_many`'s own upsert hardcodes
`file_language: verter_language::FileLanguage::vue()`
(`crates/verter_session/src/host_compile.rs:475`, inside the request build at `:469-478`). A
`.svelte` input on the public batch route is therefore parsed and compiled by the **Vue**
carrier and never reaches the Svelte one. Executed consequences, all observed:

- a supported `.svelte` component returns Vue-shaped output (`_sfc_main`, a `?vue&type=` import)
  where the single-file route returns the real Svelte module;
- NEITHER Svelte runtime refusal fires on the batch route — both `generate: "server"` and the
  advanced-rune refusal produce Vue-shaped SUCCESS where the single-file route returns the typed
  refusal;
- the same divergence appears on the host-backed batch lane, not only the render lane;
- one reviewer additionally observed the batch publishing a partial product for a refused item.

Rule: who owns this, is it one defect or two (wrong carrier selection; and refusal
non-propagation), and does it change anything about which block must gate `B2`/`B3`? Note this
is a public route returning WRONG SUCCESSFUL OUTPUT for a whole framework, which is a different
severity class from the per-cell codegen defects already dispositioned.

## Finding TR-1 — the two transports serialize a missing product differently

For the identical typed request (a `style` node on a component whose runtime surface was
refused) the in-process host returns `Err(HostError::MissingVirtualNode)`; the NAPI binding
converts it to a **null** response; the WASM binding **throws** a typed error. Both mean "no
product" and neither leaks one — that was asserted — but a consumer written against one shape
does not port to the other unchanged.

Rule: is this a defect at all, and if so whose? If it is not a defect, say what makes it
acceptable so a reviewer can check that judgement rather than re-litigate it.

## Confirm or correct the consolidated table

Every row below is `DEFER` unless you say otherwise. This repository requires a `DEFER` to carry
an architecture ruling plus a debt row naming the durable owner block, the resolution gate, and
the acceptance identifier — so your answer becomes that ruling. Correct any row you think is
wrong, including the class.

| id | finding | class | proposed owner |
|---|---|---|---|
| SV-1 | `{#each}` flags set `EACH_ITEM_REACTIVE` where official does not (21 vs 20) | compiler defect | BS0 |
| SV-2 | `$props()` non-interpolation instance-script usage falsely refused; official accepts | compiler gap | BS0 |
| SV-3 | Svelte client source map omits script-region declaration provenance | compiler defect | BS0 |
| SV-4 | untyped `$props()` destructure publishes an empty props surface, no diagnostic | session projector defect | BS0, distinct acceptance item |
| RT-1 | the batch route compiles `.svelte` as Vue and drops its refusals | ? | ? |
| AT-1 | a combined `want_ide` request publishes the TSX product after a runtime refusal | atomicity violation | BA0 |
| CSS-1 | the standalone CSS route accepts and ignores `sourcemap: true` | product defect | BCSS0 |
| TR-1 | NAPI null vs WASM throw for a missing product | ? | ? |
| RA-1 | `list_virtual_files` names `Main` for a component whose runtime surface is refused | route-assembly artifact, proposed REJECT-as-defect (the list is parse-derived and is not a publication claim; no product leaks) | — |
| RA-2 | `has_runtime_surface` includes `!styles.is_empty()`, so a refusal that started publishing CSS would take the wrong host arm — no reachable state today | latent, proposed REJECT | — |

For each `DEFER` state the resolution gate (no later than what event) and whether the named
acceptance test is the right gate. For `RA-1` and `RA-2` say whether `REJECT` is right, and if
not what they are.

Finally: given all of this, state plainly whether the block can be recommended for acceptance
now, and if not, the exact shortest set of things that must become true first — separating what
an implementer can do from what only a maintainer can ratify.
