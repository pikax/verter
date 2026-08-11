# Provenance — Revision 11 Materials

## Verified digests (SHA-256)

| artifact | digest | status |
| --- | --- | --- |
| `consolidated/verter-architecture-lock-master-plan-v11.md` | `3303834589df23cd04338801374857e685d9961df3d323c60c4b58db54ce62ce` | MATCHES the published Revision 11 validation report |
| `release/opus-orchestrator-prompt-v11.md` | `d32b3f748230b3735469195ed62e6728242774ea0a575af1999b724164a750c3` | MATCHES the published Revision 11 validation report |
| `release/validation-report-v11.md` | `027f4e6dca5798ad75066acba3bb560980f7e81103492c0fcbad75c9edc62f91` | pinned here (self-describing report; no external cross-reference exists) |
| `release/opus-start-here-v11.md` | `d1186cd49c7d7368d9ec97a998516dfba806cc0075977c1ba169cbb0d9884f28` | pinned here (matches the digest recorded in `evidence/A0-preflight-blocked.md`) |
| claimed canonical package digest (85 files) | `af11392f5f9eeea75cbd82def85adadfee41b3c8032b5248c09e96aba13123a7` | **UNVERIFIED** |

## Package validation state: UNVERIFIED, waived

The release ZIP `verter-architecture-v11.zip` and its `.sha256` were never available on
this machine, and Python 3 is not installed, so `tools/validate_package.py`,
`tools/selftest_orchestration.py`, `tools/validate_program_state.py`,
`tools/validate_stack_window.py`, and `tools/validate_landing_equivalence.py` were NOT
run. Package validation state: UNVERIFIED, waived by explicit maintainer decision
(maintainer: Carlos / `pikax`).

## Reconstruction

The 67/67 byte-verbatim attestation applies to the originally reconstructed bytes
recoverable from the digest-verified consolidated master, with **one landed-name
exception, disclosed here**: the package's canonical `README.md` was landed as
`package-README.md`, and a new repository-local index occupies `README.md`.
`ORCHESTRATOR.md` §3 makes `README.md` normative read-order item 1; readers following
that order should read `package-README.md` as the package README (the landed
`README.md` states this near its top).

AMD-002 subsequently amended the live split files `program-dag.toml`, `program.md`,
`charters/A3.md`, and `templates/program-state.template.toml`, and added
`charters/A2C.md`. AMD-003 then superseded AMD-002 points 2 through 4 and amended the
live split files `program.md`, `charters/A2C.md`, and `charters/A3.md` to materialize
the corrected completion-graph authority; it also amended the associated
`docs/arch/u6-flow-return-gaps-and-target.md` architecture note. The current bytes of
the amended or added Revision 11 split files are execution authority under AMD-002 as
superseded by AMD-003; they are not covered by the historical 67/67 byte-verbatim
claim. The pinned consolidated master remains the immutable source from which the
originally reconstructed bytes are recoverable.

## Aggregate digest of the landed split-package tree (including the repository-local index files)

The aggregate digest is computed over the LANDED files with SHA-256 (GNU coreutils
`sha256sum`), run from `docs/arch/refactor/rev11/` in a POSIX shell:

```sh
find . -type f \( -name '*.md' -o -name '*.toml' \) -not -path './consolidated/*' -not -path './release/*' -not -path './evidence/*' -not -name 'PROVENANCE.md' | LC_ALL=C sort | xargs sha256sum -b | sha256sum
```

Input set: every `.md`/`.toml` file under this directory EXCEPT `consolidated/`,
`release/`, `evidence/`, and this file. Note the set is NOT exactly "the authority
files": it INCLUDES the repository-local `README.md` index, `_EXTRACTION_INDEX.md`,
and the `amendments/` records (none of which are package files) and EXCLUDES the
consolidated canonical master. **Amendments live OUTSIDE the historical verbatim
set:** files under `amendments/` are repository-local program records, not
reconstructions of package content. Their inclusion in this aggregate does not alter
the historical 67/67 attestation. AMD-002 and AMD-003 are the disclosed exceptions
that subsequently edited the named live split files above. Each exclusion has a reason:

- `consolidated/` — the single-file canonical master is already pinned individually by
  its own SHA-256 row above (digest-matched to the published validation report);
  including its 400 KB concatenation of the same content in the aggregate would
  double-count every split file's bytes.
- `release/` — the three release artifacts are fixed published artifacts, each pinned
  individually by its own SHA-256 row above; they are not part of the reconstructed
  split tree the aggregate is meant to fingerprint.
- `evidence/` — intentionally unpinned: the evidence records change as the program
  advances (per-round record updates), so pinning them into the aggregate would churn
  it on every program transition without attesting anything about the authority
  content.
- `PROVENANCE.md` — excluded from its own input set because the digest is published
  here; a self-including digest would be unrecomputable by construction.

`sha256sum -b` pins binary-mode output (`hash *./path`
per-file lines) so the aggregate is identical across platforms whose default mode
differs. Files must be checked out with their committed LF line endings (this
repository uses `core.autocrlf=false`; a CRLF-translating checkout changes the bytes
and therefore the digest).

- File count: 73
- Aggregate digest: `4bfc008ca833b5b08875b37ee1df01426efceb89fec0430bd844d34120067cd1`

(The historical aggregate reconstruction digest
`4ab1523c4fc769cc02da61d017d7e447adf62652189350c947a3f642128d8e5c` was published
without an algorithm or input-set statement and is superseded by the recomputable
digest above; it remains quoted in the historical preflight record.)

## Not recoverable / absent

The following are NOT recoverable from the consolidated master and are therefore absent
here: `MANIFEST.json`, `VALIDATION.json`, `consolidation-order.txt`, the 9 `tools/*.py`
sources, and `agents/claude-code/*`. Consequently the 85-file manifest cannot be
reconciled against these 67 files.

The absence of `tools/*.py` also keeps this addition compliant with the repository's
no-committed-Python dependency policy.
