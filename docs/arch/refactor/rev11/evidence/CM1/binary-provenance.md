# Native binary provenance

Records which sources the shipped `@verter/native` artifact was built from, in a form that
survives the landing sequence.

## Why this binds to content and not to a commit

An earlier provenance record named a commit sha. A rebase made that sha a non-ancestor, the
record stayed well-formed while naming an identity no longer in the history, and nothing failed
or warned — it was found only when someone tried to verify it. That silence is why it survived a
whole review cycle.

A commit sha is the wrong anchor here because the landing sequence moves it twice: the landing
agent may rebase, and it will squash. Two measurements on this tree settle the shape:

- **A squash preserves the tree exactly.** A synthetic squash commit built from the branch tip's
  tree onto its merge-base produced an identical tree hash. So a tree-bound record survives a
  squash.
- **A rebase does not necessarily preserve the tree.** This branch's own pre- and post-rebase tip
  trees differed, because trunk had edited a file the branch also touches — the program ledger,
  which is not an input to the binary.

Tree-binding therefore fixes the squash half and not the rebase half. Binding to a digest over
the binary's **actual inputs** fixes both: a rebase that touches unrelated files cannot move it.
The decisive evidence is this branch's own rebase, where per-file blob identity was 45 MATCH / 0
DIFFER and the input digest below was byte-identical before and after.

**Rule:** artifact provenance binds to a digest over the artifact's inputs. A commit sha is
recorded alongside as informational only, and must never be used to verify the record.

## The binding

| | |
|---|---|
| Inputs digest | `2d6cf019925df20999b224de65a4267c8eccf7389832c362fac5a5a396cd25b2` |
| Input set | 2466 blobs — every `crates/*/src/**`, every crate manifest, workspace `Cargo.toml` and `Cargo.lock` |
| Artifact | `packages/native/dist/verter-native.darwin-arm64.node` |
| Artifact sha256 | `3605b5e971694b2325f4d6528f6d36f60d51f416a7798a1ba1044add23b58bd5` |
| Artifact bytes | 33017680 |
| Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)` |
| Builder | `@napi-rs/cli ^3.6.2`, `napi build --release --platform` |
| Build sha | `1ca3dc7bdf73f5a94cb6e44c6aa6763cb7aa79f4` — **informational only** |

## Re-deriving it

From the tree the binary was built from:

```
git ls-tree -r HEAD -- crates Cargo.toml Cargo.lock \
  | awk '$4 ~ /(\/src\/|Cargo\.(toml|lock)$)/ {print $3"  "$4}' | sort | shasum -a 256
```

The digest above was captured before the build, re-derived after it, and compared against a
capture taken before the branch's last rebase. All three agree, which independently confirms that
rebase touched no binary input.

A mismatch means the binary no longer corresponds to the sources: rebuild it and update this
record from the new artifact. The record follows the binary, never the reverse.

## Scope

Covers the darwin-arm64 artifact only, built on the machine that produced this record. The other
platform artifacts are produced by the release pipeline and are outside this block.
