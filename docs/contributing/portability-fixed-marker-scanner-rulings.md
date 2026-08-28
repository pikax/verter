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

### No exception rail

Build inputs, test inputs, generated output, source, fixtures, and documentation must not contain a
known machine marker. A hit fails. The scanner source itself is the sole intrinsic exception because it
must spell the marker set that it enforces; the exception is an exact repository path, not a directory,
suffix, basename, or documentation exemption.

Machine-bound raw logs and historical evidence stay outside the tracked repository. This replaces the
former digest-pinned evidence exception manifest, which existed only for retired roadmap backup and
source trees.

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
for known leaked roots with no tracked-content exception rail.
