# Portability fixed-marker scanner — architecture rulings

This is the durable architecture-ruling record for the tracked-file content
scanner that enforces the content-residue half of the **Cross-Platform
Portability** CRITICAL rule. It is the canonical home the guard's
`mechanism_ruling` doc-comment points at; the orchestration ledger is not a
durable enough authority for a landed-tree invariant.

## tracked-paths-no-machine-roots

### What the guard is

`tracked_paths_no_machine_roots` (in
`crates/verter_session/tests/cases/tracked_paths_no_machine_roots.rs`) is a
**fixed-marker tombstone content scanner**. It enumerates the tracked tree via
`git ls-files -z`, reads each tracked file's RAW BYTES, and fails if any
tracked file's bytes contain any of a fixed set of machine/user/session/
orchestration absolute-path markers (exact byte-subslice match, no regex, no
broadening). It is the content-residue half of the Cross-Platform Portability
rule; the path-SHAPE half (NTFS-legal components, no case collisions,
≤200-byte paths) lives in the sibling `tracked_paths_are_portable`. Both walk
the same `git ls-files -z` enumeration.

The scan is byte-level, not lossy-UTF-8: every marker is pure ASCII and can
appear verbatim inside an otherwise-binary or one-stray-byte-non-UTF-8 tracked
blob, and gating the scan on `from_utf8` would silently skip exactly those
files. Reads are fail-closed (an unreadable tracked file is a guard failure,
not a skip), and the path enumeration is fail-closed on path encoding too (a
non-UTF-8 tracked path is a guard failure, matching the sibling guard's claim).

### Why a scanner (Structural-Confinement-First)

A tracked-file-TEXT content-residue invariant cannot be expressed by any
compiler or structural mechanism: the offending bytes are arbitrary string
literals inside source, docs, JSON fixtures, and skill files — there is no
type, trait, module boundary, or build-graph edge that "ownership of a leaked
absolute path" maps onto. Per Structural-Confinement-First, a structural
mechanism is preferred wherever one exists; none exists here, so a fixed-marker
tree scanner is the correct, recorded, justified mechanism for this invariant.

### Marker-set provenance

The set is **64 fixed markers**. It grew from a base set through three bounded
expansions, each authorized by a neutral architecture consult recorded in this
ruling. None of the three broadened the scanner into the legitimate fixture
set.

- **Base 13 markers** — the originally-classified machine/user/session/
  orchestration roots: one developer's POSIX `$HOME`, the macOS Claude
  scratch dir, the two `/tmp`-rooted orchestration-scratch roots (the named
  orchestration-ledger and orchestrator scratch dirs), another developer's
  Windows Claude personal-config directory (the `.claude` dir under that
  user's Windows home), in both forward-slash and backslash spellings, the
  developer's personal checkout root in its Windows-drive and WSL-mount
  spellings, and that drive's Windows scratch root in upper- and lower-case
  drive spellings.

- **Expansion A (markers 14-26)** — the separator-equivalent spellings
  (Windows-backslash / mixed-separator / UNC) of the already-classified
  checkout-root tail and the `D:`/`d:` drive scratch. Slash direction is only
  spelling, so each separator variant that contains at least one backslash is
  the same machine-specific path. Same machine roots, no new class, no
  broadening into the legitimate fixture set, and deliberately no broad
  `D:/dev`.

- **Expansion B (markers 27-62)** — the same developer's SCOPED `dev/wt`
  (worktree-root) and `dev/temp` (sandbox-scratch) siblings of the checkout
  root, across drive / separator / Git-Bash / WSL spellings, each bounded by a
  TRAILING separator so a marker cannot substring-match an unrelated path that
  merely shares the prefix text. Scoped known roots only — deliberately NOT a
  broad `D:/dev` ban, which would false-positive the legitimate `dev/project`
  / `dev/example` fixtures. The discrimination negatives for `dev/project` and
  `dev/example` pin the boundary.

- **Expansion C (markers 63-64)** — the lowercase-drive spellings of the
  already-classified Windows Claude personal-config root (markers #6/#7) — the
  same `.claude` directory with a lowercase drive letter, in forward-slash and
  backslash spellings.
  Drive-letter case is spelling only for this root: the path/URI normalization
  layer canonicalizes Windows drive letters to lowercase, and the marker set
  already treats drive-case twins as same-family. No new username, no new
  directory class, no broad `c:/Users` coverage — the marker still requires the
  exact `.claude` segment, so it does not touch the legitimate generic Windows
  path/URI fixture families (`c:/Users/dev`, `c:/Users/david/workspace`). This
  expansion was authorized by a reopened Structural-Confinement architecture
  review as a bounded case-normalization completion.

The exact byte spelling of all 64 markers is pinned in the `MACHINE_MARKERS`
const and the `constructed_markers_equal_intended_bytes` set-pin in the guard
source `crates/verter_session/tests/cases/tracked_paths_no_machine_roots.rs`;
this ruling intentionally does not re-list the literal spellings so that the
ruling file itself does not embed a banned marker (the guard's own tree-scan
treats this docs file like any other tracked file — it is not allowlisted).

### Terminal state

Three expansions is the bound reached for this scanner. No further marker
additions — same-class or otherwise — are made without reopening the
Structural-Confinement decision through the architecture rail. A same-class
future discovery is fixed in the offending files (or ignored by git for future
local tool state), never appended to the marker set without that ruling.

### Documented residual

This is intentionally NOT a complete machine-path detector. A complete detector
would false-positive the ~70 legitimate cross-platform path/URI fixtures the
repo deliberately carries (generic `c:/Users/dev`, `/Users/Foo/Bar.vue`,
`/home/runner`, `C:/tmp/Foo.vue.ts`, `C:\tmp\Foo.vue.ts`, `D:\dev\project`,
`d:/dev/example`, Linux-CI `/tmp`). The following machine-local classes are
DELIBERATELY uncaught:

- a NEW, different developer's `$HOME`;
- a third username;
- a different drive letter's scratch (e.g. `E:\tmp\…`);
- a bare-parent `dev` root (`D:\dev` with no `wt` / `temp` / `personal` tail);
- the SSR scratch `C:/temp/` class (fixed in the offending files, not
  marker-guarded);
- any other machine-local path not in the fixed list.

These are handled by in-file fixes plus a gitignore policy for future local
tool state, not by widening the scanner. Widening the scanner to cover the
home / scratch / bare-parent class broadly (`/Users/`, `C:/Users/`, `/home/`,
`/tmp/`, `[A-Za-z]:\tmp\`, `[A-Za-z]:\dev`) would false-positive the legitimate
fixture families above, which is why the scanner stays a fixed-marker tombstone
for the known roots rather than a general detector.
