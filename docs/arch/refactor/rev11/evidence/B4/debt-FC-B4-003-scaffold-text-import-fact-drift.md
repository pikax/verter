# Tracked debt — FC-B4-003: scaffold-text/import-fact drift in `assemble_vue_main_module`

Disposition: **DEFER** (per CLAUDE.md "Explicit finding disposition"; adversarial-testing
finding from the round-3 review, recorded per that ruling rather than fixed in this round —
this round is explicitly a targeted mechanical delta, not another architecture pass).

## What happened

`publish`'s undeclared-helper check (`crates/verter_compiler/src/assembly/publish.rs:219-230`)
exists to catch a composer whose reported `ArtifactContribution.emitted_imports` disagrees
with what its own `fragments` actually declare — i.e. code that uses a helper the fragment
inventory never declared importing. At `assemble_vue_main_module`'s own call site
(`crates/verter_session/src/compile.rs:599-607`), `emitted_imports` is built by cloning the
SAME `fragments` list the contribution also carries:

```rust
let emitted_imports: Vec<DeclaredImport> = fragments
    .iter()
    .flat_map(|f| f.fragment().imports.iter().cloned())
    .collect();
let contribution = ArtifactContribution {
    kind: planned_kind,
    fragments: fragment_refs,
    code: sequenced.code,
    emitted_imports,
    ...
};
```

and `publish`'s check (`publish.rs:220`) reads `declared_names(&contribution.fragments, ...)` —
the SAME `fragments`. Both sides of the check are literally the same data at this one call
site, so the check is structurally tautological here: it cannot catch composed SCAFFOLD TEXT
(the hand-written prelude/trailer strings this function builds directly, e.g. the HMR
registration block, the `__file` assignment, the terminal `export default`) drifting from the
declared `DeclaredImport` fact list, because nothing about the scaffold TEXT is compared
against anything independent of the facts it was never derived from in the first place.

**Verified concretely.** Pushing an import specifier string directly into one of this
function's scaffold `String` buffers (e.g. the prelude) with no corresponding
`DeclaredImport` pushed onto that same fragment's `imports` list sails through `publish`
uncaught — the composed artifact's actual bytes and its declared import facts can silently
diverge, and nothing in this call path notices.

**This is a real, narrow gap, not a stub or a gate-bypass.** The check still does genuine
work as designed: it protects against a DIFFERENT composer (any future or existing caller of
`assembly::publish` other than this one) misreporting `emitted_imports` relative to its own
`fragments` — e.g. a composer that receives fragments from one source and independently
computes its emitted-imports list from another. `assemble_vue_main_module` is simply the one
call site where those two inputs happen to be sourced from the same collection, making the
check a no-op specifically there.

## Ruling reference

Round-3 (final) adversarial review, finding 4: *"a real, narrow architectural gap found by
adversarial testing... Record it as a new debt row... disposition DEFER, owner B4... This is
a real, narrow gap, not a stub/gate-bypass, and this is the final round — do not attempt a
redesign now."*

## Owner

B4 / `verter_compiler::assembly` — a future B4-adjacent pass. This is assembly-substrate
territory (how one composer derives its own `emitted_imports` from its own scaffold text), not
BV1 (Vue chunk producers), BS1, or B5 semantic work.

## Resolution gate

Either of the following closes this row:

(a) **Single source of truth.** Derive scaffold text FROM the `DeclaredImport` list at
    composition time — i.e. the prelude/trailer `import`/HMR/`__file` lines are generated
    FROM the same fact list `emitted_imports` reports, rather than the fact list and the text
    being written independently by hand in parallel. This eliminates the possibility of drift
    structurally: there is only one place the import identity is spelled out.

(b) **A production check that the composed artifact's actual import statements exactly match
    its declared facts.** E.g., a final-parse-adjacent pass over the composed module's actual
    `import` declarations (already available once the module is parsed for the final-parse
    check `publish.rs` already runs) compared against `contribution.emitted_imports`, refusing
    publication on any mismatch in either direction (an import in the bytes with no declared
    fact, or a declared fact with no corresponding import in the bytes).

Either resolution needs a discriminating regression test proving a planted scaffold-text/fact
drift is caught (a mutation-plant proof, not just a clean-input pass) before this row closes.

No code changes accompany this record — `assemble_vue_main_module`'s scaffold-building and
`emitted_imports` derivation are unchanged from before this finding.

## Acceptance ID

No existing acceptance id in `capability-matrix.tsv` (or elsewhere in the framework-
conformance evidence tree) covers this internal drift-detection gap. Minting **`FC-B4-003`**
as this debt row's own acceptance id.
