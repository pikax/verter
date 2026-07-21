# Engine provisioning — Tier 3 (download) is unimplemented and blocked on an owner decision

**Status:** BLOCKED — needs a dependency/supply-chain decision from the product owner.
**Owner decision required before any implementation.**

## Symptom

Verter cannot obtain a TypeScript engine on a machine that does not already have one.

The ratified provisioning policy has five tiers:

```
0  VERTER_TSGO_BIN, then PATH          implemented, correct
1  hook into the editor's tsgo          fixed (see fix/w5-engine)
2  node_modules: tsgo OR tsserver       fixed (see fix/w5-engine)
3  download latest supported minor      NOT IMPLEMENTED  ← this document
4  Verter's own bundled engine          NOT IMPLEMENTED  ← separate document
```

Tiers 3 and 4 are the only ones that work when the user's project supplies nothing usable.
With both missing, a project whose toolchain predates TypeScript 7 depends entirely on
tier 2's tsserver fallback, and a project with no TypeScript at all gets nothing.

## Mechanism

The temp cache is **read-only by construction**. `enumerate_cache_tier` reads
`verter-tsgo-v1/<user>/<triple>/<policy>/<version>/`
(`crates/verter_tsgo_api/src/toolchain/discovery.rs:421-482`) and the module documents the
tier as "consume-only". Nothing anywhere in the repository writes it — no HTTP client, no
`download`/`fetch`/`install` function in `verter_tsgo_api`. The cache directory has never
existed on any observed machine.

The read side is already hardened and would accept a correct writer: symlink/reparse-point
rejection, cache-root ownership checks, and group/world-writable refusal all live at
`crates/verter_tsgo_api/src/toolchain/discovery.rs:501-571`.

## Why this is blocked rather than merely unfinished

The Rust workspace has **no HTTP client and no TLS stack** (`hyper` appears only as a
transitive `tower-lsp` dependency). Implementing this tier means adding a network
dependency whose output is a **binary that Verter then executes**.

That is a supply-chain and architecture decision, not an implementation detail:

- it introduces a network dependency into a previously network-free resolution path;
- it adds a TLS/HTTP dependency tree to the workspace;
- the artifact it produces is executed, so integrity verification is a security control,
  not a nicety;
- it changes Verter's behaviour on a developer machine from "use what you have" to
  "fetch what you need", which may be unacceptable in some environments (air-gapped,
  audited, corporate proxy).

## The design that was ready to ship

Recorded so the decision can be made on a concrete proposal rather than in the abstract.

A `verter_tsgo_api::toolchain::provision` module that:

1. Resolves the newest **stable** version inside `SUPPORTED_TSGO_RANGE` from the npm
   registry for `@typescript/typescript-<os>-<arch>`.
2. Downloads to `<cache>/verter-tsgo-v1/<user>/<triple>/<policy>/.tmp-<pid>-<rand>/`.
3. Verifies the registry `dist.integrity` SHA-512 **before** unpacking.
4. `create_dir_all`s the version directory with `0o700` on Unix, so it satisfies the
   existing `unix_cache_root_trust_issues` check.
5. Atomically `rename`s the temp directory into `<version>/`.
6. Writes `READY.json` **last** — the existing reader already treats a missing marker as an
   incomplete install, which *is* the concurrent-writer guard: losers of the rename race
   simply discard their temp directory.
7. Never follows symlinks on any written component.

That satisfies every existing read-side check without modifying the reader.

## What a decision looks like

- **Approve** ⇒ implement as designed; the dependency addition must respect the repository
  dependency policy in `CLAUDE.md`.
- **Reject** ⇒ tier 3 is permanently absent; the policy document should be amended to a
  four-tier policy so the gap is not repeatedly rediscovered, and tier 4 (bundled sidecar)
  becomes the sole offline floor — which makes resolving that document's blocker
  correspondingly more important.
- **Defer** ⇒ record the fallback behaviour users should expect in the interim: tier 2's
  tsserver fallback carries every project that ships any TypeScript, and projects with none
  fail closed with an honest status.

## Blast radius

Nothing depends on tier 3 today because it has never worked. Implementing it changes
first-run behaviour on machines with no engine; not implementing it leaves those machines
without TypeScript semantics, honestly reported.
