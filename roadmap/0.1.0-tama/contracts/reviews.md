# Review contract

Use the risk-scaled review profile named by the node charter. Review the squashed candidate patch, consolidate findings, apply fixes, rerun the required review lenses, and run the final gate.

Before review starts, the candidate patch must already have transitioned its predeclared ledger line from pending to implemented with the planned squash message, approximate timezone-bearing date, and optional PR number. The transitioned row is not review proof; it keeps implementation state and the candidate patch together.

Review tasks remain fresh and independent where the profile requires it. They do not need SHA-bound manifests, immutable-tree receipts, prompt/report digests, or runtime registration. A candidate change after review still calls for judgment: rerun affected review and verification in proportion to the change, without restamping identities.

Do not rebuild the retired validation system around report filenames, task IDs, Git objects, or GitHub metadata.

Surviving findings follow `FindingCarryForward` in `github-control-plane.md`. P0/P1 block; issue closure is not resolution.

Node review is not the only review scope. A train receives a fresh Codex Architect conformance review after each tranche of 3 to 6 implemented blocks, no later than before a seventh unchecked block proceeds. The train's final intended block also triggers a separate fresh cumulative train review. That final review covers all implemented train outcomes and the final candidate against current authority, including every ordinary reviewed amendment effective for the train. Neither train-level review replaces the final block's own review profile; material fixes require the affected cumulative lens to rerun.
