# Script-test registration fragments

`pnpm test:scripts` and `pnpm test:scripts:ci` resolve their commands from
`scripts/run-script-tests.mjs`, which reads every `*.json` fragment in this
directory in byte-wise filename order. A node that adds a script test adds its
own fragment here and does not edit root `package.json`.

## Format

```json
{
  "owner": "<task slug or area>",
  "tests": [
    {
      "runner": "node-test",
      "file": "scripts/my-thing.test.mjs",
      "ci": true
    }
  ]
}
```

- `owner` — the task slug or area that owns the registrations.
- `runner` — one of:
  - `node-test` — runs `node --test <file>`; an optional `nodeFlags` array
    prepends node flags (e.g. `["--experimental-strip-types"]` for the
    sfc-projection TypeScript specs).
  - `vitest` — runs `vitest run <file>`.
  - `node` — runs `node <file>`, for a sub-runner such as REG0's
    `scripts/run-framework-locks.mjs`.
  - `pnpm` — runs `pnpm run <script>`; `script` must exist in the root
    `package.json` (used by `test:generic-interface-identity-oracle`).
- `file` — a plain repo-relative path; the file must exist. A file registered
  by two entries is rejected, as is a `pnpm` script registered twice.
- `ci` — `true` makes the entry part of `pnpm test:scripts:ci`.
- `ciIndex` (optional) — explicit position in the CI lane: indexed entries run
  in `ciIndex` order, then unindexed entries follow in fragment order. Only
  `00-baseline.json` uses it: the replaced CI chain ran
  `scripts/observe-lane.test.mjs` second although it sits late in the full
  chain, and `ciIndex` preserves that exact order. New fragments normally omit
  it and simply append to the CI lane.

The loader rejects an unknown runner, a missing file, a file registered
twice, a fragment without `owner`, and malformed JSON — each for its own
reason. `node scripts/run-script-tests.mjs --list` (or `--ci --list`) prints
the resolved command lines without running them.
