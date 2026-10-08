# Universal-tooling constitution

This decision fixes the dependency directions and the boundaries between the
universal kernel, the horizontal products, the verticals, the project profiles
and the optional compilers. Today, framework-shaped host/session registries and
untagged public boundaries own these concerns. The final and sole owner is the
typed immutable universal catalog and the demand-selected kernel services.

It describes the repository at `docs(arch): record the kernel authority
inventory and deletion ledger`, 2026-10-08. It follows the docs-only rule in
[README.md](README.md): it changes no production route and adds no check. It
builds on the [authority inventory](authority-inventory.md) and does not
re-own anything that inventory assigns.

## Machine-readable products

The reviewed contract data lives in `tests/kernel/UAK1/products/`:

| File | Holds |
| ---- | ----- |
| `constitution-inventory.v1.json` | Layers `L0`–`LR`, firewall rules `F01`–`F05`, constitution outcomes `U01`–`U12`, the UAK0 outcomes this decision relies on, consumers `K-C01`–`K-C04`, empty populations and transferred obligations |
| `firewall-route-ledger.v1.json` | Firewall breaches at the described head (`K01`, `K02`) with one deletion owner each, plus UAK0's `D01`–`D19` mapped to the rule each one breaches |

Every owner is an existing plan node. Every `successorPath` starts at UAK1 and
follows predecessor edges in the controller-owned DAG at dispatch. The UAK0
rows (`B..`, `O..`, `C..`, `S..`, `D..`) keep their UAK0 owners; UAK1 only
references them.

## Layers

| Layer | Role | Holds | May import |
| ----- | ---- | ----- | ---------- |
| `L0` | kernel | Identity and source: `verter_identity`, `verter_span`, the registered source authority in `verter_language`, `verter_execution` and the leaf marker crates | nothing above `L0` |
| `L1` | kernel | Kernel services: the one type-resolution engine, retained source lowering, neutral query DTOs, resolution, scheduler, workspace, `verter_protocol`, and `verter_session` apart from its composition root | `L0` |
| `L2` | product | Horizontal products: TypeInfo/ComponentInfo, workspace index, diagnostics and native checker, lint/rules/actions, formatter, public request/result envelope | `L0`, `L1` |
| `L3` | vertical | Carrier frontends and their semantic (framework) profiles, one family module each | `L0`–`L2` |
| `L4` | project profile | Project-profile overlays | `L0`–`L3` |
| `LC` | compiler backend | Optional runtime compiler backends, registered per vertical | `L0`, `L1`, its own `L3` frontend |
| `LH` | host / presentation | Editor hosts and clients, CLI command adapters and reporters, NAPI/WASM/FFI/MCP bindings | anything except routing by framework name (see `F04`) |
| `LR` | registration | The one composition root that builds the `CatalogSnapshot` | `L0`–`L4`, `LC` |

The layer is a role, not a crate. Today several roles share one crate:
`verter_compiler` holds both carrier frontends (`L3`) and runtime backends
(`LC`), and `verter_session` holds kernel services, the framework registry and
per-family modules. The rules below bind the roles. Splitting crates is the
receiving owner's choice, as long as the rule holds.

## Dependency firewall

- **F01.** No kernel layer (`L0`, `L1`) imports a vertical, project-profile,
  editor-host, CLI-presentation or compiler-backend owner.
- **F02.** A horizontal product imports no vertical, project profile, host or
  backend. It learns vertical facts only from catalog-registered
  contributions.
- **F03.** A carrier frontend never imports a compiler backend. A backend may
  import its own frontend.
- **F04.** Only the composition root names concrete verticals, project
  profiles and backends. Kernel services, products and hosts select them by
  catalog identity, never by framework spelling or a closed per-framework
  branch.
- **F05.** A breach present at the described head is an equality-pinned
  exception that names its deletion route. The exception set only shrinks,
  and it shrinks in the change that lands that route's owner.

UAM0 makes `F01`–`F05` executable (`UAM0-AC-R1`), in
`crates/verter_source_policy_gate/tests/cases/kernel_dependency_firewall.rs`.
The check must be structural (cargo metadata closures, visibility), not a
name scanner. It extends two guards that already hold part of the rule:
`workspace_dependency_layers` (`crates/verter_identity/tests/cases/`) and the
ARH1 contracts in `architecture_dependencies.rs`.

## Rules

### 1. No runtime, no compiler creep

- The kernel never executes project or framework code. JS-host packages run
  inside the framework's own process and capture configuration there; Rust and
  WASM consume captured facts only. Dynamic evaluation needs an explicitly
  authorized execution service that produces a new captured snapshot
  (CFG0). Owner: CENV1C (`U01`, `CENV1C-AC2`).
- Every carrier has a frontend. Only compile-capable carriers register an
  optional compiler backend. "No compiler" is a normal state, never an error
  path of tooling, and the frontend imports no runtime codegen. Owner: CPF1
  (`U02`, `CPF1-AC2`; contract CPF0).
- Compiler capability is declared per exact vertical release. It is never
  inferred from tooling support, and parser/compiler conflation is a manifest
  structural failure. Owner: VIM1 (`U03`, `VIM1-AC2`; contract VIM0).
- Tooling never pulls in project optimization; that stays with the compiler
  program (CMP0, OPT0).

### 2. Carrier → semantic profile → project profile

- A **carrier profile** owns syntax: bytes, geometry, parse and recovery.
- A **semantic (framework) profile** owns the meaning of one exact framework
  release over one or more carriers. One carrier byte stream may carry several
  mutually exclusive semantic claims, and a profile activates only on its
  exact release. Owner: VID0T (`U04`, `VID0T-AC3`); the identities themselves
  are UAK0 `O01`.
- A **project profile** overlays semantic profiles through generic roles,
  realms and generated facts. It never selects a TypeScript program (that is
  the project-bound external-TS contract), never owns resolution (PM), never
  mutates a carrier or semantic identity, and does no work when inapplicable.
  Owner: PPR1 (`U05`, `PPR1-AC2`; contract PPR0). Derived project identity is
  UAK0 `O10`.

Each layer depends only on the one before it. No universal framework IR sits
between them, and no single parser implementation is required.

### 3. Public capability truth and partial outcomes

- Every public result carries one typed outcome: success, partial, ambiguous,
  NeedInputs, unsupported, not-applicable, cancelled or stale. The vocabulary
  is the same on Rust, NAPI, WASM, LSP, MCP and CLI. A surface that lacks
  inputs reports NeedInputs, never empty success. Owner: UAP0 (`U06`,
  `UAP0-AC-R2`; contract PUB0).
- The per-surface capability/maturity matrix is generated from manifests and
  freshness-checked. There are no boolean capability lies and no registered
  no-op handlers. Owner: VIM1 (`U07`, `VIM1-AC1`).
- Partial, cancelled, stale or ambiguous outcomes never warm a cache. This is
  the existing cache rule, kept unchanged.

### 4. Independent terminals and continuous soak joins

- Every vertical and every horizontal product has its own terminal. No global
  release join exists, and one vertical's progress never gates another
  vertical's release. Owner of the cross-family check: UAK2 (`U08`,
  `UAK2-AC1`).
- The old release universe, with its global `EXT0`/`TVG0`/`PJG0`/`X1`
  coupling, is superseded. It has no successor node.
- A soak join is continuous evidence over accepted terminals. It fixes
  nothing, gates no release and revokes no accepted terminal. A failure
  becomes a non-invalidating follow-up for the exact owner. Reference join:
  CEJ0 (`U09`, `CEJ0-AC1`).
- An architecture-falsification join (UAK2, UKS0, CMP6) may precede a product
  only to falsify a shared abstraction, never as a release gate.

### 5. Static registration, no dynamic plugin ABI

- Carriers, semantic profiles, project profiles and backends register
  statically in Rust at the composition root, into one immutable
  `CatalogSnapshot` (UAK0 `O02`, CPF1). Nothing is registered or replaced
  after construction.
- There is no runtime plugin loading, no dynamic native ABI, and no `Any` in
  public registration.
- Extensions are data-only manifests or isolated guest execution behind a
  versioned manifest. They cannot replace core identity, type semantics,
  resolver, cache validity or edit-transaction authority, and first-loaded or
  fastest-returning never wins a conflict. Owner: XSDK1 (`U10`,
  `XSDK1-AC2`; contract XSDK0).

### 6. Adoption

This decision is adopted through its `architecture-3` review and maintainer
landing. No identity-bound receipt is recorded.

## Forbidden designs

- A universal framework IR, or a requirement that every vertical use one parser
  implementation.
- A project profile that selects or creates a TypeScript program.
- A compiler capability inferred from tooling support.
- A dual-running authority, compatibility fallback, string/regex semantic
  recovery, or unqualified cache or public identity.
- Defining a universal kernel contract by naming a future framework. Every rule
  above is stated without one. Framework names appear only as counterexamples
  or as current-head facts, so the abort clause does not apply.

## Firewall breaches at the described head

| Route | Breach | Rule | Deletion owner |
| ----- | ------ | ---- | -------------- |
| `K01` | `verter_session`'s production closure links the compiler's runtime/compile backends (`compile`, `compile_transaction`, `assembly`, `standalone`, `svelte::runtime`, `style_planner`, `framework_common::vue_bridge`). `workspace_dependency_layers` ranks the compiler beneath the session. | `F01` | CPF1 (`CPF1-AC1`) |
| `K02` | The Svelte JSX shim assets are embedded in the kernel crate (`framework/svelte_jsx_assets`). `verter_lsp` and `verter_tsc` read them from there. | `F01` | CPF1 (`CPF1-AC1`) |
| `D01`–`D19` | UAK0 routes, each mapped in the ledger to the rule it breaches | `F01`, `F02`, `F04`, `U05`, `U06` | UAK0 owners |

No charter names removing the `K01` crate edge explicitly. CPF1 is recorded
as its owner because CPF1 installs the optional backend registry, migrates the
compile and IDE-projection routes, and must leave a frontend that imports no
runtime codegen (CPF0's acceptance). Operator question
`uak1-kernel-compiler-edge-owner` asks whether to keep CPF1 or add a dedicated
firewall-cutover node.

Empty populations at this head:

- **Dynamic plugin ABI.** No production crate depends on a dynamic-loading or
  plugin-host crate. The only `libloading` in `Cargo.lock` comes from
  `napi-sys`, which binds Node-API symbols and loads no Verter plugin.
  Framework adapters are trait objects built in-process at host construction.
- **Embedded JavaScript runtime.** No production crate depends on a JavaScript
  engine, and Rust/WASM execute no project configuration.
- **Project-profile knowledge in kernel crates.** `verter_identity`,
  `verter_language`, `verter_protocol` and `verter_session` have no Nuxt or
  SvelteKit branch.
- **CLI presentation in kernel crates.** `verter_session`'s three binaries are
  feature-gated test tooling outside the production closure.

## Findings recorded for the receiving owners

- At dispatch, CMP6 (non-release compiler falsification, including a Solid 2
  slice) precedes the product terminals BND5, NUX3 and PPR1. Rule 4 allows a
  falsification join before a product only to falsify a shared abstraction.
  UAK2 confirms the edge is not a release gate (`UAK2-AC1`).
- At dispatch, VST1 is labelled non-release but is a Vue-owned capability that
  LSO10, NUX3 and PPR1 consume. It is a capability dependency, not a soak
  join, so rule 4 does not apply to it.

## Acceptance evidence

Evidence selection: the change adds contract text and data only, so existing
coverage and bounded inspection discriminate it. The diff adds no test.

- **AC1 — ownership contract.** The two product files bind every constitution
  outcome, consumer and firewall breach to one existing node, a successor
  path from UAK1 and a receiving acceptance ID. UAK0's rows are referenced,
  not re-owned, so no route has two owners. The executable validator and its
  negative controls are UAM0's (`UAM0-AC-R1`). Acyclicity and the absence of a
  global release join are properties of the controller-owned DAG, which this
  repository does not carry.
- **AC2 — positive contract.** The named boundaries' identity, provenance and
  ordering behaviour is pinned by the coverage UAK0 cites. The dependency
  directions are already held in part by `workspace_dependency_layers`, by the
  ARH1 contracts in `arh12_dependency_and_visibility_contracts_are_enforced`,
  and by `no_cross_product_binary_imports`, `lsp_mcp_dependency_direction`,
  `lsp_binary_compile_graph_cannot_reach_verter_mcp` and
  `no_verter_semantic_to_verter_session_dep`. New tests belong to UAM0.
- **AC3 — incremental equivalence: not applicable.** No cache, cancellation,
  stale-publication or partial-result authority is touched, and no production
  byte changes.
- **AC4 — bounded work: not applicable.** No hot path changes. Zero work for
  inapplicable profiles is UAK0's `Z01`, confirmed by PER0E.
