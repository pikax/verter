# Review contract

Use the risk-scaled review profile named by the node charter. Review the squashed candidate patch, consolidate findings, apply fixes, rerun the required review lenses, and run the final gate.

Before review starts, the candidate patch must already contain its `[[implemented]]` ledger row with the planned squash message, approximate timezone-bearing date, and optional PR number. The row is not review proof; it keeps implementation state and the candidate patch together.

Review tasks remain fresh and independent where the profile requires it. They do not need SHA-bound manifests, immutable-tree receipts, prompt/report digests, or runtime registration. A candidate change after review still calls for judgment: rerun affected review and verification in proportion to the change, without restamping identities.

Do not rebuild the retired validation system around report filenames, task IDs, Git objects, or GitHub metadata.

Surviving findings follow `FindingCarryForward` in `github-control-plane.md`. P0/P1 block; issue closure is not resolution.

