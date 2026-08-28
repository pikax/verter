# Portability fixed-marker scanner — architecture rulings

This is the current-lineage architecture record for the tracked-file content scanner enforcing the
content-residue half of the **Cross-Platform Portability** CRITICAL rule. The guard's
`mechanism_ruling` comment points here. This file restores the durable mechanism record on this lineage;
it does not claim that a similarly named document from a divergent historical commit was present here.

## tracked-paths-no-machine-roots

### Mechanism

`tracked_paths_no_machine_roots`, at
`crates/verter_source_policy_gate/tests/cases/tracked_paths_no_machine_roots.rs`, is a fixed-marker
tombstone scanner. It enumerates the tracked tree with `git ls-files -z`, reads every tracked file's raw
bytes, and reports any exact occurrence of the guard's closed set of known machine, user, session, or
orchestration roots. The sibling `tracked_paths_are_portable` owns path shape: valid UTF-8, portable
components, case-collision freedom, and the length ceiling.

The scan is byte-level rather than lossy UTF-8 because each marker is ASCII and can occur inside an
otherwise non-UTF-8 blob. File reads and tracked-path decoding fail closed. This is executable enforcement
of arbitrary tracked content; no compiler type, capability boundary, or dependency edge can express the
same property, so a fixed-content scanner is the structurally appropriate mechanism.

### Two portability rails

The operative invariant distinguishes portable repository content from exact authority evidence:

1. Build inputs, test inputs, runtime behavior, generated output, source, fixtures, and portable
   documentation must not contain a known machine marker. A hit fails.
2. Exact architecture evidence may preserve the environment where it was produced only through
   `scripts/manifests/portability-machine-marker-evidence-exceptions.tsv`. The file remains scanned and a hit is
   admitted only by exact repository path, exact worktree SHA-256, an existing pin document containing
   that digest, a permitted authority-evidence or ruling root, and a manifest digest pinned by
   this mechanism record. Exception manifest SHA-256: `17ac6be4c2db353105ba96832e005fbf99fed7f8687336ba84af8a7ebb8077e1`.

There is no exemption for `docs/`, `evidence/`, rulings generally, suffixes, basenames, or
"non-production" content. Wildcards, duplicate rows, malformed digests, missing pins, and targets outside
the class-specific roots are invalid. The audit-only review-history class is confined to
`docs/arch/refactor/rev11/sources/review-history-migration/` and each row is pinned by
`authority/state/historical-review-audit.json`. Liveness is bidirectional: a marker with no row fails, and a row whose
marker is gone is stale and fails until the owning authority retires it.

Future machine-bound raw logs default to external digest-bound bundles. The in-tree exception rail exists
for the exact registered audit-backup evidence and inherited ruling inputs; it is not
the default storage policy for new logs.

### Marker-set provenance and bound

The guard source pins the exact 64-marker set and its reconstruction test. The set reached that bound by
three reviewed expansions: separator-equivalent spellings of already classified roots; scoped worktree and
sandbox sub-roots with trailing-separator bounds; and lowercase-drive spellings of one already classified
personal configuration root. Those expansions did not broaden into the repository's representative
cross-platform fixture families.

Sixty-four markers and three hardening rounds remain the terminal bound. A new marker class or another
same-class spelling requires a reopened architecture decision. Otherwise the offending portable content
is fixed or local tool state is ignored rather than silently widening the scanner.

### Residual

This scanner does not claim to detect every absolute path. Broad home, temporary-directory, drive, or
checkout-prefix detection would reject legitimate cross-platform path and URI fixtures. It is a tombstone
for known leaked roots, paired with exact integrity admission for the narrow authority-evidence set.
