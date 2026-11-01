# Engine artifact contract evidence index

Selected case IDs: `EPR1-AC1`, `EPR1-AC2`, `EPR1-AC3`, `EPR1-AC4`, `EPR1-AC-IDENTITY`, `EPR1-AC-COMPAT`, `EPR1-AC-INTEGRITY`, `EPR1-AC-INSTALL`, `EPR1-AC-REVOCATION`, `EPR1-AC-SOLE`, `EPR1-AC-CONTRACT`, `EPR1-AC-INCREMENTAL`, `EPR1-AC-WORK`.

Commands (run on the candidate; this file is not an execution transcript):

- `node --test tests/engine-provisioning/EPR1/engine-contract.test.mjs`, in `pnpm run test:scripts:ci`
- `node tests/engine-provisioning/EPR1/tools/validate-engine-validation-ownership.mjs --structural`
- `node tests/engine-provisioning/EPR1/tools/validate-engine-validation-ownership.mjs --live`, wherever the plan controller is reachable; it fails rather than passes without one
- `pnpm --filter docs check`, changed-file `pnpm exec oxfmt --check`, and `pnpm exec vitest run packages/vue-vscode/src/package.spec.ts`

| Case                  | Contract proof in `engine-contract.test.mjs`                               | Runtime proof, not yet executed                                   |
| --------------------- | -------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| `EPR1-AC1`            | Schema, full population coverage, one owner per member, live resolution    | Each row's receiving acceptance in the catalog                    |
| `EPR1-AC-SOLE`        | Each refusal code, each starting from the complete inventory               | Planted-route guards in the receiving owners                      |
| `EPR1-AC2`            | Fixtures for every definition, omissions and added claims refused, goldens | `EPR4-AC-CONTRACT`                                                |
| `EPR1-AC-CONTRACT`    | Same, plus typed rejection rows and the redaction projection               | `EPR4-AC-CONTRACT`, `EPR5-AC-CAPABILITY`                          |
| `EPR1-AC-IDENTITY`    | Axis mutations never alias; key survives locator and field order           | `EPR4-AC-CANDIDATES`                                              |
| `EPR1-AC-COMPAT`      | Declaration mutations and version boundaries reach every verdict           | `EPR4-AC-CANDIDATES`                                              |
| `EPR1-AC-INTEGRITY`   | Byte, origin, signature and manifest mutations against recomputed digests  | `EPR4-AC-CANDIDATES`, `EPR2-AC-VERIFY-FIRST`, `EPR3-AC-SUPPLY`    |
| `EPR1-AC-INSTALL`     | State machine, crash points, reader checks, writers, extraction bounds     | `EPR2-AC-SAFE-EXTRACT`, `EPR3-AC-INSTALLED`, `EPR4-AC-CANDIDATES` |
| `EPR1-AC-REVOCATION`  | One action per stage and event; none reuses or falls back                  | `EPR4-AC-CANDIDATES`, `EPR4-AC-CACHE`, `EPR5-AC-REVALIDATE`       |
| `EPR1-AC3`            | Every key field forces fresh validation; admission table                   | `EPR4-AC-CACHE`, `EPR4-AC-INCREMENTAL`                            |
| `EPR1-AC-INCREMENTAL` | Unchanged reuse against stale, cancelled, partial and changed-basis        | `EPR4-AC-INCREMENTAL`, `EPR5-AC-INCREMENTAL`                      |
| `EPR1-AC4`            | Finite limits, complete counters, zero-work modes with owners              | `EPR4-AC-WORK`, `EPR2-AC-WORK`, `EPR3-AC-WORK`, `EPR5-AC-WORK`    |
| `EPR1-AC-WORK`        | Same; the warm control is declared and not run                             | `EPR4-AC-WORK`                                                    |

Preflight rationale: no runtime hot path, cache or publication changes, so no soak or performance measurement applies; the vectors bind later owners to the behaviour they must test. Characterization was read-only over the sources named by the catalog's displaced rows. No production route is deleted or certified here, and no install abort condition applies because every supported operating system has the primitives the product lists.
