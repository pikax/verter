# Program ledger — imported copy

This directory is a **transport copy** of the live program ledger and its evidence bundles,
imported so the program can be resumed on another machine. The authoritative live ledger is kept
outside any checkout by maintainer ruling; this copy exists to move state between machines, not
to become the ledger's home.

## This directory is deleted from history at plan close

**Obligation:** when the program completes, this directory is removed from the repository *and
from git history* — not merely deleted in a later commit, which would leave every blob in every
clone forever.

The cheapest way to honour that is to keep the importing commit **off `main`**: it lives only on
the program branch, and when the program lands, it lands without this commit. Deleting the branch
then makes the objects unreachable and no rewrite is needed.

If the commit ever does reach a long-lived branch, removal requires a history rewrite:

```
git filter-repo --path docs/arch/architecture-lock/ledger --invert-paths
```

followed by a force-push, expiring reflogs, and repacking. Note that a force-push does **not**
immediately erase the objects on the forge: unreachable objects stay fetchable by direct SHA until
the forge garbage-collects, which may require asking the forge operator to run it.

## What was imported

314 files, about 10 MB. Everything under the evidence root except:

- `target/`, `.git/`, `node_modules/` — build output and repository metadata;
- `mutation-worktree/`, `parent-check/` — throwaway clones created for mutation and parent checks;
- `scratch/` — full repository checkouts captured during gate comparisons, which duplicate the
  product tree;
- two compiled `.exe` benchmark artifacts under `A2C/command-proofs/`. Their digests are still
  recorded in `ORIGINAL-DIGESTS.tsv`, and the digest indexes that referenced them note the
  dropped entries, so their absence is documented rather than silent.

## Paths were normalized, and here is how to verify that

The repository carries a fail-closed guard that reads the raw bytes of every tracked file and
rejects a fixed set of machine-specific absolute-path roots. Evidence captured on a developer
machine is full of them, and that guard's own policy is that such roots are fixed in-file rather
than added to an allowlist. So absolute roots were replaced with stable placeholders:

| Placeholder | Replaced |
|---|---|
| `<REPO>` | the repository checkout root, in every separator spelling |
| `<EVIDENCE>` | the external evidence root |
| `<HOME>` | a developer home directory |
| `<CLAUDE_DIR>` | a developer tool-state directory |
| `<TMP>` | machine scratch roots |

`ORIGINAL-DIGESTS.tsv` records every imported file's SHA-256 **before** normalization, one
`<relative-path>\t<sha256>` row per file. Anyone can therefore confirm that a committed copy
differs from its original only by this path substitution, by fetching the original and comparing.

Two classes of digest were recomputed against the normalized copies so this tree is
self-consistent:

- the `*.sha256` indexes, which are designed to verify in place — all three verify with
  `sha256sum -c` returning exit 0 in this tree;
- `context_packet_digest` and `evidence_digest` in `program-state.toml`, for the two blocks whose
  referenced records contained machine paths.

Digests quoted in prose inside the `index.md` files were **not** rewritten. They refer to the
original pre-normalization bytes and remain checkable against `ORIGINAL-DIGESTS.tsv`.

## Using this copy

Live-mode validation runs against the copy here:

```
node scripts/validate-program-state.mjs \
  --dag docs/arch/refactor/rev11/program-dag.toml \
  --state docs/arch/architecture-lock/ledger/program-state.toml \
  --mode live
```

The block-authorization registry check is mandatory in live mode, not opt-in: the command above
already enforces it, with no extra flag, because its default path (`authority-registry.toml` next
to `--state`) resolves to `authority-registry.toml` in this same directory. Pass `--authority
<path>` only to point at a different registry, or the explicit `--no-authority` to opt out (never
the default — see the validator's own usage text).

If you record a transition on one machine, the copy on the other machine is stale. This directory
has no merge story — treat one machine as the writer at a time, or reconcile by hand.
