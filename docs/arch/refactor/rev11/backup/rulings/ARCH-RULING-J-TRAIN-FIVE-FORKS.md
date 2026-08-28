---
ruling_id: "J-TRAIN-FIVE-FORKS"
type: "architecture-ruling"
date: "2026-08-20"
date_source: "stated"
binds: ["J1", "J2", "J3", "J4"]
source_file: "ARCH-RULING-J-TRAIN-FIVE-FORKS.md"
summary: "Codex rules on the five forks blocking J1-J4 charter drafting: (1) J4's parser-coverage / no-fallback / no-duplicate-grammar evidence MOVES INTO J1, since J1 cannot delete the legacy route before the replacement's Native coverage is proven; J4 keeps the capability matrix, recovery and handoff proof. (2) The generic source-map composition engine is a NEW layer-2 crate `verter_source_map`, not CodeTransform, not verter_identity, not verter_compiler; J3 converges the compiler/session map representations into it. (3) External-preprocessor EXECUTION authority is JS/builder-only via a round-trip protocol; Rust never invokes external tooling; the Rust-LSP baseline has no preprocessor provider, so preprocessing-dependent results are External and unavailable there. (4) NO DAG EDGE CHANGES — J2 and J3 already have what they need, B4's mapping substrate reaches J3 transitively; but J1 must be PROMOTED from class subsystem to foundational. (5) J1 is PARITY-GATED for every currently retained Native operation; Unsupported is not an acceptable temporary landing state, and SCSS/Sass/Less/Stylus lowering is External, not Unsupported."
supersedes: []
superseded_by: []
contradicts: []
notes: "Rejects two premises: Lightning CSS is NOT a fallback selected after StyleSyntaxIr failure (it is a separate legacy processStyle parser/normalizer, and the SFC compiler already uses StyleSyntaxIr planners) though the sequencing problem is still real; and 'Rust or JS call direction' is a false binary (data crosses both ways, execution authority is JS-only). Records four genuine open questions the directive leaves undecided, including the product x dialect x operation compatibility matrix (still VERIFY) and final runtime CSS Modules ownership. The J1 class promotion is a program-dag.toml change and requires a formal amendment; it is NOT applied by this ruling."
---

# Architecture Ruling — Track J five forks

**Status:** RATIFIED 2026-08-20 by the codex architect.

Consulted read-only against `program/architecture-lock`. The forks were posed by
the program orchestrator after the maintainer ratified
[`MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER`](MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md),
which updates the Track J plan but does not itself resolve the forks that block
charter drafting.

The verdict is recorded verbatim below.

---

Ruling: choose **1(b)**; create a shared source-map crate; keep preprocessing execution in JS; assert **no DAG edge changes**.

### 1. J1/J4 sequencing

**Decision:** Move J4’s parser-coverage/no-fallback/no-duplicate-grammar evidence into J1. J4 retains the closed capability matrix, recovery, and preprocessor-handoff proof.

J1 cannot remove the old authority at [MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:365](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:365>) and defer proof that Native syntax is accepted until J4 at [same:400](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:400>). Native means unparseable valid syntax is a bug, with no fallback permitted ([same:274](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:274>)).

Move these J4 bullets to J1 acceptance:

- five-dialect parser coverage;
- absence of private duplicate grammar;
- no valid Native syntax dependent on Lightning CSS or another parser.

J1 also proves stage-local `CodeTransform` mappings; generic multi-stage composition remains J3. J4 consumes J1’s proven Native set and proves recovery, External/Unsupported behavior, and handoff.

The current evidence is insufficient to assume parity: the capability contract says no dedicated standalone CSS selector exists beyond limited package/conformance coverage ([capability-matrix.md:67](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/capability-matrix.md:67>)).

### 2. Generic source-map composition owner

**Decision:** Create a layer-2 shared crate, `verter_source_map`.

It owns:

- qualified source-space/artifact graph;
- ordinary map-stage validation and composition;
- multi-source preservation;
- unmapped-segment semantics;
- on-demand terminal flattening/encoding.

`CodeTransform` remains the stage-local edit/map producer. `verter_identity` retains map identity types only; it explicitly does not construct or encode maps ([mapping.rs:1](<<MACHINE_ROOT>/verter/crates/verter_identity/src/mapping.rs:1>)). `verter_compiler` is too high-level because the session already carries a parallel qualified-map model ([verter_session/src/types.rs:2179](<<MACHINE_ROOT>/verter/crates/verter_session/src/types.rs:2179>)). `verter_span` remains coordinate primitives, not a composition engine ([verter_span/src/lib.rs:1](<<MACHINE_ROOT>/verter/crates/verter_span/src/lib.rs:1>)).

This follows the directive’s source-space, multi-source, unmapped-region contract ([directive:188](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:188>), [directive:238](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:238>)) and B4’s ownership of generic mapping composition ([B4.md:5](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/B4.md:5>)).

**J3 charter consequence:** extract/converge the compiler/session map representations into this crate; do not build a CSS composer or widen `CodeTransform::chain_source_map`.

### 3. External-preprocessor boundary

**Decision:** Execution authority is JS/builder-only, using a round-trip protocol:

```text
Rust parses authored style and emits request/prepared input
→ JS/builder invokes external preprocessor
→ JS/builder submits sealed result
→ Rust parses returned plain CSS and applies Vue transforms
```

Rust does not invoke arbitrary JS, processes, or filesystem tooling. The directive states that the builder invokes tooling and feeds the sealed result back ([directive:259](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:259>)). The current host already has this direction ([verter_session/src/types.rs:1804](<<MACHINE_ROOT>/verter/crates/verter_session/src/types.rs:1804>), [unplugin/src/index.ts:358](<<MACHINE_ROOT>/verter/packages/unplugin/src/index.ts:358>)).

J1 must remove both current deviations:

- Vite’s JS `compileStyleAsync()` currently implements Vue scoping/`v-bind()` ([unplugin/src/index.ts:826](<<MACHINE_ROOT>/verter/packages/unplugin/src/index.ts:826>));
- non-Vite uses the legacy standalone `processStyle` path ([same:856](<<MACHINE_ROOT>/verter/packages/unplugin/src/index.ts:856>)).

For the LSP: `.vue` IDE compilation remains available without a preprocessor because `CompileTarget::IDE` is TSX-only ([verter_session/src/types.rs:1323](<<MACHINE_ROOT>/verter/crates/verter_session/src/types.rs:1323>)). Rust may analyze authored CSS-family syntax natively, but preprocessing-dependent results are `External` and unavailable unless a JS client/provider supplies a sealed result. The LSP must not silently preprocess internally.

### 4. DAG predecessors

**Decision:** No edge changes.

- J2 already has exactly what processor/config identity needs: B1 identities, B2 parsed identities, and J1’s preprocessor boundary ([program-dag.toml:333](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/program-dag.toml:333>)). Processor/version/config are opaque supplied identity inputs; J2 must not derive them by invoking tooling.
- J3 already transitively follows B4’s mapping-composition substrate: `B4 → BV1/BS1 → B5 → B6 → J3` ([program-dag.toml:130](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/program-dag.toml:130>), [same:160](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/program-dag.toml:160>), [same:340](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/program-dag.toml:340>)). Adding a direct B4 edge would be redundant.

Non-edge correction: J1 must be promoted from `subsystem` to **foundational**, because it changes shared syntax ownership and public compatibility—the template’s explicit trigger ([J1.template.md:4](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/J1.template.md:4>)).

### 5. J1 capability floor

**Decision:** J1 is parity-gated for every currently retained Native operation. `Unsupported` is not an acceptable temporary landing state.

Minimum J1 floor:

- Native parsing/fact extraction: CSS, SCSS, Sass, Less, Stylus.
- Native authored `v-bind()` analysis/transform where proven safe.
- Native Vue scoped/deep/global/slotted/keyframe transforms on plain CSS after any required external preprocessing.
- SCSS/Sass/Less/Stylus lowering: `External`, not `Unsupported`.
- Runtime CSS Modules may be External; semantic module-class analysis remains Native.

An `Unsupported` final state is acceptable only for an operation explicitly outside Verter’s responsibility or explicitly removed as a ratified breaking change. The directive already authorizes removal of normalized-byte compatibility and standalone `processStyle` ([directive:321](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:321>)). It does not authorize silently dropping retained end-to-end dialect support.

### Rejected premises

- Lightning CSS is not currently a fallback selected after `StyleSyntaxIr` failure. It is a separate legacy `processStyle` parser/normalizer ([css/mod.rs:61](<<MACHINE_ROOT>/verter/crates/verter_compiler/src/css/mod.rs:61>)); the SFC compiler already uses `StyleSyntaxIr` planners ([compile/mod.rs:842](<<MACHINE_ROOT>/verter/crates/verter_compiler/src/compile/mod.rs:842>)). The sequencing problem is still real because J1 deletes a user-facing legacy route before replacement capability is proven.
- “Rust or JS call direction” is a false binary: data crosses both directions, but external-tool execution authority is JS-only.
- A new shared map crate does not imply a new DAG predecessor.

### Genuine open questions

- The exact current product×dialect×operation compatibility matrix remains `VERIFY` ([capability-matrix.md:22](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/capability-matrix.md:22>)); J1 must inventory and ratify it.
- Final runtime CSS Modules ownership is deliberately undecided ([directive:309](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:309>)).
- The canonical processor/config fingerprint schema and the host admission that binds upstream map `sources` to authoritative source-space identities are unspecified.
- No compatibility pre-preprocessor rewrite is pre-approved; each dialect/preprocessor case requires evidence ([directive:138](<<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:138>)).
- Whether a future JS LSP client supplies external preprocessor results is undecided; the Rust-LSP baseline is no such provider.
