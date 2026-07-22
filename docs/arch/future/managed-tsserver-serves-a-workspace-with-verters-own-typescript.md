# The managed tsserver tier can serve a project with Verter's OWN TypeScript

Recorded 2026-07-22 by the VS Code acceptance lane while validating route
selection. Distinct from the two neighbouring records: this is not "the engine
answers nothing", it is "the engine is the wrong engine, and the status says
otherwise".

## Symptom

Open a package-manager workspace whose TypeScript is installed per package
rather than hoisted to the workspace root (a pnpm workspace is the normal case;
private corpus F has this shape — a workspace root with no root
`node_modules/typescript`, four configured projects under `packages/*`, each
with its own TypeScript). Let `auto` fall through to the managed tier.

The status bar reports a connected engine and the reason reads:

```
no supported tsgo engine is available for <workspace>, falling back to the
workspace tsserver <VERTER-REPO>/packages/vue-vscode/node_modules/typescript/lib/tsserver.js
```

The path is **not in the user's workspace at all** — it is the TypeScript that
ships with the Verter extension. The user's project is then type-served by a
TypeScript install it never chose, with that install's `lib.*.d.ts`, version
semantics and module-resolution root, while the status calls it "the workspace
tsserver".

Measured on corpus F, `VERTER_ACCEPTANCE_PROVIDER=auto`, debug `verter-lsp`,
shared machine, 3 repeats per probe:

| operation | carrier (`.vue`) | native TypeScript yardstick (same session) |
| --- | --- | --- |
| hover | 0 / 51 carrying TypeScript, 27 Verter-native, 10 empty; p50 **68 ms** | 18 / 39 carrying quickinfo; p50 8 ms |
| definition | 3 / 33 resolved, 30 empty; p50 **2362 ms** | 33 / 33 resolved; p50 2359 ms |
| completion | **0 / 15 resolved** (15 unresolved); p50 18 ms, p95 5875 ms | 15 / 15 resolved; p50 5862 ms |
| references | 33 / 36 resolved; p50 3 ms | 24 / 24 resolved; p50 10 ms |

The contrast that isolates it: on corpus D — same lane, same binary, same
machine shape, but a workspace with a **root** `node_modules/typescript` — the
same code path resolves the project's OWN tsserver and the reason names a path
inside the user's workspace.

## Mechanism

`find_tsserver` (`crates/verter_type_runtime/src/discovery.rs:52-93`) has three
tiers, in order:

1. `<workspace_root>/node_modules/typescript/lib/tsserver.js`, walking **up**
   through parent directories (10 levels).
2. `<tsdk>/tsserver.js`.
3. Global TypeScript via `npm root -g`.

Tier 1 only ever walks **upward**. In a workspace whose TypeScript lives in
`packages/<name>/node_modules/typescript`, nothing is found: the walk starts at
the root and every ancestor is outside the project.

Tier 2 then fires, and `tsdk` is never empty: the extension always supplies one
(`packages/vue-vscode/src/extension.ts:1538-1548`):

```ts
const userTsdk = verterConfig.get<string>("typescript.tsdk", "");
// Always pass --tsdk: user setting → bundled TypeScript (fallback for pnpm strict mode etc.)
const tsdk = userTsdk || bundledTsdk;
args.push(`--tsdk=${tsdk}`);
```

So the fallback for "the user did not pin a tsdk" is the extension's own
TypeScript, and `probe_managed_engine` (`crates/verter_lsp/src/main.rs:664-695`)
hands that path to `choose_managed_engine`, which formats it into the
user-facing reason as "the workspace tsserver"
(`crates/verter_lsp/src/main.rs:596-601`).

Two separate defects sit on top of each other:

1. **Tier 2 of the ratified provisioning order is not actually implemented for
   this workspace shape.** "If `node_modules` provides an engine, use it" is not
   satisfied by an upward-only walk from the workspace root when the engine is
   installed per package.
2. **A tier-4-style bundled engine is reported as a tier-2 workspace engine.**
   `engine-provisioning-spec.md` explicitly requires: *"The resolved engine and
   the reason for it must be reported honestly in status and logs."* A bundled
   fallback presented as the workspace tsserver is exactly the false-provenance
   case that rule exists to prevent.

## Reproduction

Fully synthetic; no private corpus needed.

1. Create a workspace with `packages/a/tsconfig.json` and
   `packages/a/node_modules/typescript` installed, and **no**
   `node_modules/typescript` at the workspace root.
2. `pnpm --filter verter-vscode run prepare:e2e`
3. Point the acceptance lane at it with `VERTER_ACCEPTANCE_PROVIDER=auto` and no
   tsgo engine available.
4. Read `provider.reason` in the receipt: it names a `tsserver.js` under the
   Verter extension, not under the workspace.

A unit-level reproduction is smaller still: call
`verter_type_runtime::discovery::find_tsserver(Some("<extension>/node_modules/typescript/lib"), Some("<workspace-root>"))`
on the layout above and observe that it returns the extension's path.

## Evidence

Inline above. The receipt's `provider.reason` string is the primary artifact and
it is self-evidencing — it prints the absolute path of the selected engine.

## Why deferred

Two reasons, both from `SCOPE.md`:

- The correct fix requires a **design decision this workstream does not own**:
  in a monorepo with several per-package TypeScript installs, which one serves a
  given carrier? The honest answer is per-configured-project engine selection,
  which is a provisioning-tier change owned by `engine-provisioning-spec.md`,
  not a benign local fix.
- Changing which engine is selected can turn currently-working setups into
  `None`. That is a behaviour change well beyond "removing the defect".

## Proposed fix and falsifiable prediction

Two independent changes:

1. **Resolve tier 2 from the configured project, not only from the workspace
   root.** For a carrier, walk up from its owning tsconfig's directory before
   falling back to the workspace root walk. That is what `tsc` and tsserver
   themselves do, and it is what "the project's own TypeScript" means in a
   monorepo.
2. **Report provenance honestly.** `ManagedEngineFacts` should carry WHERE the
   tsserver came from (project walk / operator tsdk / bundled), and
   `choose_managed_engine` should say so. A bundled engine is a legitimate tier
   4; calling it "the workspace tsserver" is not.

Falsifiable prediction: on the synthetic layout above, `provider.reason` names
`packages/a/node_modules/typescript/lib/tsserver.js`. On corpus F, the reason
names a path inside the corpus. If the reason still names the extension's
TypeScript, the fix has not worked.

Discriminating test: a unit test over `find_tsserver` with a per-package layout,
asserting the returned path is under the workspace and NOT the supplied tsdk —
it fails today for the original reason (tier 1 misses, tier 2 wins).

## Blast radius

**If fixed:** projects of this shape start being served by their own TypeScript.
Diagnostics may change (different `lib`, different version), which is correct but
visible. A workspace with no TypeScript anywhere would move from "silently
served by Verter's copy" to an honest bundled-tier report — better, but it is a
status change users will notice.

**If left alone:** every pnpm-style workspace that reaches the managed tier is
type-served by whatever TypeScript the extension happens to ship, and the status
bar asserts otherwise. Any bug report from such a project is unreproducible,
because the reporter's TypeScript version is not the one that produced the
result.

**Interaction:** this contaminates measurements of the managed tier on
workspaces of that shape — a managed-tier latency or correctness number taken
there is a number about the extension's TypeScript, not the project's. Any
comparison between the managed tier and another tier on such a workspace must
state this.
