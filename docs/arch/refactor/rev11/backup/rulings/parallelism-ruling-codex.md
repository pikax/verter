---
ruling_id: "BF3-PARALLELISM"
type: "architecture-ruling"
date: "2026-08-16"
date_source: "file-mtime (no in-document date)"
binds: ["BF3", "J1"]
source_file: "parallelism-ruling-codex.md"
summary: "Rules nothing may run and land concurrently with BF3 — only non-landing, read-only preparatory work (investigation, inventories, draft charters/evidence) is legal. J1 fails the safety bar against BF3 (same Svelte compiler pipeline: CSS plan / CodeTransform mappings / StyleSyntaxIr are shared, not disjoint). Corrects the ledger: BF3 is recorded READY, not IN_PROGRESS, with no context-packet digest bound — governance requires an accepted context packet before execution starts."
supersedes: []
superseded_by: []
contradicts: []
notes: "Companion document to B2-scope-and-concurrency-ruling-codex-1.md and B3-scope-ruling-codex-1.md."
---

d locked ([ledger:1055](<MACHINE_ROOT>/verter/docs/arch/architecture-lock/ledger/program-state.toml:1055)).

That is dependency eligibility, not necessarily a missed unlock. J1 has only a template, explicitly optional ([J1 template:3](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/J1.template.md:3)); it lacks the ratified charter/context packet required for `READY`. Before ratification, the orchestrator must prove parallel closures disjoint ([governance:156](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/governance.md:156)).

J1 fails your safety bar against BF3:

- BF3 must withhold JavaScript, CSS, diagnostics, maps, declarations, and other products atomically ([BF3:13](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/BF3.md:13)).
- J1 inventories and potentially reconciles CSS syntax, Vue/Svelte transforms, `CodeTransform` mappings, preprocessors, and public contracts ([J1:7](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/J1.template.md:7)).
- These are concretely the same Svelte compiler pipeline: `compile_client` both consumes the CSS plan ([client_compile.rs:140](<MACHINE_ROOT>/verter/crates/verter_compiler/src/svelte/runtime/client_compile.rs:140)) and constructs the published CSS/map artifact ([client_compile.rs:236](<MACHINE_ROOT>/verter/crates/verter_compiler/src/svelte/runtime/client_compile.rs:236)); that plan directly consumes `StyleSyntaxIr` ([css/mod.rs:36](<MACHINE_ROOT>/verter/crates/verter_compiler/src/svelte/runtime/css/mod.rs:36)). This is not a very-low-conflict boundary.

Separate parallel windows must declare shared files, and discovered overlap stops or serializes them ([stacked-prs:60](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/stacked-prs.md:60)). Whichever candidate lands second would also require restacking and revalidation after the base advances ([stacked-prs:104](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/stacked-prs.md:104)). Therefore criterion (b) is not highly probable either.

2. Preparatory work

Governance does permit successor charters to be pre-scoped ([implementation-lock template:103](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/templates/implementation-lock-record.md:103)). Thus read-only investigation, source inventories, and draft charters/evidence are legal.

It does not permit implementation, a begun ledger state, or ratified/frozen candidate evidence before predecessors are accepted, except through a validated stack ([governance:6](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/governance.md:6)). Any committed preparation landing before BF3 would advance BF3’s base and trigger restack/revalidation, so it would not satisfy your fast-forward bar.

3. BF3’s B2/B3 clause

It binds B2/B3 block dispatch—implementation or other begun block execution—not out-of-block pre-scoping. That is the only reading consistent with both “no B2/B3 dispatch” ([BF3:42](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/BF3.md:42)) and the explicit permission to pre-scope successors.

Bottom line: no other block should be exposed or landed concurrently. Only non-landing preparatory thought work is legitimate.
tokens used
175 593
No. Nothing qualifies to run and land concurrently with BF3. BF3 is a genuine serialization point for program execution, though not for read-only planning.

One correction: the ledger records BF3 as `READY`, not `IN_PROGRESS`, with no context-packet digest, base, candidate, or stack binding ([ledger:342](<MACHINE_ROOT>/verter/docs/arch/architecture-lock/ledger/program-state.toml:342)). Governance requires validation before a block starts, and execution requires an accepted context packet ([governance:181](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/governance.md:181), [agent-orchestration:57](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/agent-orchestration.md:57)). The counts do match: 12 accepted, 1 superseded, 1 ready, 44 locked.

1. Missed unlock

J1 is the sole `LOCKED` block whose DAG predecessors are all accepted: its predecessors are A4 and A6 ([DAG:291](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/program-dag.toml:291)), while its ledger row remains blank and locked ([ledger:1055](<MACHINE_ROOT>/verter/docs/arch/architecture-lock/ledger/program-state.toml:1055)).

That is dependency eligibility, not necessarily a missed unlock. J1 has only a template, explicitly optional ([J1 template:3](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/J1.template.md:3)); it lacks the ratified charter/context packet required for `READY`. Before ratification, the orchestrator must prove parallel closures disjoint ([governance:156](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/governance.md:156)).

J1 fails your safety bar against BF3:

- BF3 must withhold JavaScript, CSS, diagnostics, maps, declarations, and other products atomically ([BF3:13](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/BF3.md:13)).
- J1 inventories and potentially reconciles CSS syntax, Vue/Svelte transforms, `CodeTransform` mappings, preprocessors, and public contracts ([J1:7](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/J1.template.md:7)).
- These are concretely the same Svelte compiler pipeline: `compile_client` both consumes the CSS plan ([client_compile.rs:140](<MACHINE_ROOT>/verter/crates/verter_compiler/src/svelte/runtime/client_compile.rs:140)) and constructs the published CSS/map artifact ([client_compile.rs:236](<MACHINE_ROOT>/verter/crates/verter_compiler/src/svelte/runtime/client_compile.rs:236)); that plan directly consumes `StyleSyntaxIr` ([css/mod.rs:36](<MACHINE_ROOT>/verter/crates/verter_compiler/src/svelte/runtime/css/mod.rs:36)). This is not a very-low-conflict boundary.

Separate parallel windows must declare shared files, and discovered overlap stops or serializes them ([stacked-prs:60](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/stacked-prs.md:60)). Whichever candidate lands second would also require restacking and revalidation after the base advances ([stacked-prs:104](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/stacked-prs.md:104)). Therefore criterion (b) is not highly probable either.

2. Preparatory work

Governance does permit successor charters to be pre-scoped ([implementation-lock template:103](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/templates/implementation-lock-record.md:103)). Thus read-only investigation, source inventories, and draft charters/evidence are legal.

It does not permit implementation, a begun ledger state, or ratified/frozen candidate evidence before predecessors are accepted, except through a validated stack ([governance:6](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/governance.md:6)). Any committed preparation landing before BF3 would advance BF3’s base and trigger restack/revalidation, so it would not satisfy your fast-forward bar.

3. BF3’s B2/B3 clause

It binds B2/B3 block dispatch—implementation or other begun block execution—not out-of-block pre-scoping. That is the only reading consistent with both “no B2/B3 dispatch” ([BF3:42](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/BF3.md:42)) and the explicit permission to pre-scope successors.

Bottom line: no other block should be exposed or landed concurrently. Only non-landing preparatory thought work is legitimate.
