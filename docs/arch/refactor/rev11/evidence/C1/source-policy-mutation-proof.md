# C1 source-policy authority-evidence mutation proof

- Starting trunk SHA: `570c8f34660df8c354a0325e524f0d46e402e1fe`
- Guard: `crates/verter_source_policy_gate/tests/cases/tracked_paths_no_machine_roots.rs`
- Command: `cargo test -p verter_source_policy_gate --test main tracked_paths_no_machine_roots --no-fail-fast`
- Result: PASS, 12/12 tests.

The rail mutations are hermetic temporary-directory plants. Each starts from `valid_rail_fixture`, whose
exact path, evidence digest, pin document, permitted root, manifest digest, and live marker validate GREEN.
The mutation is then applied only to the temporary fixture and the production validator must return the
named RED error. Dropping the fixture is the restore; the real-tree selector is the unplanted GREEN control.

| Test | Reversible mutation | Required RED |
|---|---|---|
| `evidence_exception_admits_only_the_exact_registered_bytes` | Change only admitted evidence bytes | Evidence digest mismatch |
| `evidence_exception_manifest_digest_is_ratified` | Change only manifest bytes after the ruling pin is written | Manifest digest mismatch |
| `unlisted_marker_bearing_evidence_is_rejected` | Add a marker-bearing evidence file with no row | One portability violation |
| `listed_path_outside_permitted_roots_is_rejected` | Move the exact row target outside the evidence/ruling roots | Permitted-root refusal |
| `wildcard_and_duplicate_exception_paths_are_rejected` | Replace an exact path with a wildcard; then duplicate an exact row | Wildcard refusal; duplicate-path refusal |
| `malformed_digest_and_missing_pin_document_are_rejected` | Replace the digest with non-SHA text; then name an absent pin document | Malformed-digest refusal; missing-pin refusal |
| `evidence_exception_rows_are_live_in_both_directions` | Delete the marker but retain the row; then restore the marker and remove its row | Stale-row refusal; marker violation |

Controls and integration results:

- Complete scanner family: PASS, 12/12.
- Exact selector: PASS, 1/1.
- Complete `verter_source_policy_gate` package: PASS, 187/187, zero skipped.
- All nine admitted worktree SHA-256 values match their manifest and pre-existing authority pins exactly.
