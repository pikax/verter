# Repository cache fit

Measured 2026-11-02, 02:19 to 08:09 UTC, on hosted runner caches for `pikax/verter`. **Outcome: undeclared. `all-fit` is ruled out; `partial-fit` is not established, and cache adoption stays held.** Occupancy and per-identity sizes below are service-recorded compressed bytes.

- The 26 candidate identities measured total 13,464,887,303 bytes (13.46 GB), more than the allowance before any other consumer is counted. 14 further declared identities have no recorded entry, so `all-fit` is ruled out twice over. This is the one firm result.
- No subset is certified. Preserved occupancy (everything that stays regardless of keying) was sampled hourly, and the peak sample came from a listing cut short, so its listed figure is a lower bound. The service reported 11,878,088,196 bytes at that moment against 10,627,965,985 listed: 1,250,122,211 bytes were counted but never attributed to an entry. Preserved occupancy then was between 8,603,424,650 and 9,853,546,861 bytes.
- Against the upper bound, the subset that held at every sampled moment is **one** identity at 10 GB (`detect-changes`, 81,333,794 bytes, 65,119,345 left) and four at 10 GiB. Against the lower bound the same rule admits six at 10 GB and eight at 10 GiB (1,331,965,445 and 1,905,809,562 bytes), but that is a ceiling on the space available, not headroom, so those identities are **provisional**. Neither subset covers a peak between samples, which stayed unobserved: sccache grew from 523 to more than 5,000 entries within four hours.
- Occupancy was 9,838,709,590 bytes at the last complete listing, so headroom was 161,290,410 bytes as 10 GB, or 898,708,650 bytes as 10 GiB. It reached 11,878,088,196 bytes during the window, above both readings, so at that moment there was no headroom at all.
- The largest consumer is not a target cache. sccache's all-ref exception held at least 7,292,045,956 bytes in one listing. That one exception is why so little budget remains for keyed targets.

This measures the current job-scoped keys on the standard hosted runners. It changes no key, runner, permission or retention setting, and it buys no capacity or storage. A consolidated per-profile key is not one of these rows: its archive has no recorded size until it is built and saved, so an adopted shared identity must be measured again rather than estimated from these rows.

## Method and access

[`scripts/ci-cache-fit.mjs`](https://github.com/pikax/verter/blob/main/scripts/ci-cache-fit.mjs) issues only GET requests to [GitHub's cache REST API](https://docs.github.com/en/rest/actions/cache) and classifies each key by consumer. Its `report` command derives the declared identities from `.github/workflows` rather than from a list kept here. `size_in_bytes` is the stored archive size, not an uncompressed target directory.

This repository is public, so the listing and usage endpoints answered without a credential. That replaces the [inventory](README.md)'s receipts for those two endpoints, which came from a job token that GitHub refuses with HTTP 401. Other read access was refused:

| Read-only request                                             | Result                        | Consequence                                                                         |
| ------------------------------------------------------------- | ----------------------------- | ----------------------------------------------------------------------------------- |
| `actions/cache/usage`, anonymous                              | 200                           | Repository bytes and count                                                          |
| `actions/caches`, anonymous                                   | 200, 60 requests an hour      | Every entry's key, ref, size and times                                              |
| `actions/cache/storage-limit`, anonymous                      | 401                           | **The configured limit is unavailable.** Both readings of 10 GB are evaluated below |
| `actions/jobs/<id>/logs`, `actions/runs/<id>/logs`, anonymous | 403, `Must have admin rights` | A save's log line cannot size an entry that eviction already removed                |

The anonymous allowance is shared with everything else on the worker's address, and a repository listing can need more than 50 pages. Of 82 main-ref samples, 73 returned no complete listing, as did 2 of the 6 repository listings tabulated below (the command under [Reproduce](#reproduce) takes six). Each failure is recorded and retried only at the next scheduled sample. A listing cut short still records the size of every entry it read, but its total is only a lower bound, and it is never taken as current occupancy. The raw observation log from this window is not kept in the repository, so the tables below are the retained record of it; a rerun produces a fresh log and report.

## Occupancy over the window

Times are UTC. "Preserved" is everything that stays regardless of keying: sccache, the VS Code host, package-manager caches, and any candidate saved on a non-main ref. The lower figure is what the listing attributed. The upper figure adds the usage the service counted but the listing never attributed (usage minus listed bytes, never below zero, since usage and listing are read moments apart); it equals the lower figure on a complete listing. On complete listings, the listed sum equals the reported usage to the byte. The reported entry count ran 20 or 21 below the listed count on those listings (for example 514 against 534); the listing is used for counts.

| Time  | Listing   | Usage bytes    | Listed bytes   | Entries | sccache GB | VS Code GB | Package GB | Candidates GB | Preserved bytes (lower) | Preserved bytes (upper) |
| ----- | --------- | -------------- | -------------- | ------- | ---------- | ---------- | ---------- | ------------- | ----------------------- | ----------------------- |
| 02:19 | complete  | 10,147,358,875 | 10,147,358,875 | 534     | 2.55       | 0.33       | 0.98       | 6.28          | 3,863,737,442           | 3,863,737,442           |
| 03:19 | complete  | 10,736,546,249 | 10,736,546,249 | 2382    | 4.85       | 1.31       | 0.98       | 3.59          | 7,145,955,670           | 7,145,955,670           |
| 04:20 | cut short | 11,023,928,697 | 11,058,052,234 | 1146    | 1.82       | 0.33       | 0.98       | 7.92          | 3,133,790,504           | 3,133,790,504           |
| 05:20 | complete  | 10,472,634,721 | 10,472,634,721 | 2902    | 5.35       | 1.31       | 0.98       | 2.83          | 7,641,184,167           | 7,641,184,167           |
| 06:20 | cut short | 11,878,088,196 | 10,627,965,985 | 4699    | 7.29       | 0.33       | 0.98       | 2.02          | 8,603,424,650           | 9,853,546,861           |
| 07:20 | complete  | 9,838,709,590  | 9,838,709,590  | 1155    | 2.51       | 0.33       | 0.98       | 6.02          | 3,817,474,579           | 3,817,474,579           |

Current occupancy by consumer, from the last complete listing, with each consumer's peak across all listings:

| Consumer             | Bytes         | Entries | On main       | On other refs | Peak bytes    |
| -------------------- | ------------- | ------- | ------------- | ------------- | ------------- |
| compile-contracts    | 4,371,355,615 | 3       | 4,371,355,615 | 0             | 4,656,247,958 |
| sccache              | 2,506,093,578 | 1145    | 1,229,569,717 | 1,276,523,861 | 7,292,045,956 |
| rust-target          | 1,156,050,965 | 3       | 1,156,050,965 | 0             | 4,222,896,550 |
| node-package-manager | 983,113,978   | 2       | 983,113,978   | 0             | 983,113,978   |
| explicit-cargo       | 493,828,431   | 1       | 493,828,431   | 0             | 493,828,431   |
| vscode-host          | 328,267,023   | 1       | 0             | 328,267,023   | 1,313,079,979 |

What this shows beyond the totals:

- **Occupancy is not held under the allowance.** The service reported 11,878,088,196 bytes at 06:20 and 9,838,709,590 at 07:20. Eviction runs after a save, so a momentary reading can exceed the limit.
- **sccache sets the pace of eviction.** Its entries went from 523 to more than 5,000 in four hours, and at 07:20 the oldest surviving sccache entry was about an hour old. It writes from every ref, as the [CI cache policy](https://github.com/pikax/verter/blob/main/.github/workflows/ci.yml#L16) documents.
- **The VS Code exception holds several copies.** One 328 MB entry is scoped to each ref, and four were resident together (1,313,079,979 bytes), rather than the single entry per pinned version the policy comment describes.
- **Saved target caches do not stay long.** None of the Rust target entries resident at 02:19 was still resident at 05:20. Of the 26 measured identities, 8 were resident in the last complete listing. Fitting within the allowance does not by itself retain an entry, because eviction is by last access across all consumers.

## Size per identity

Sizes are each identity's newest save on main. "Env" is the rust-cache environment hash: the same job name under a different toolchain or environment is a different build, so `svelte-compiler-benchmark` appears twice. "Saves" counts distinct saves seen, and older ones are not added to the size. "Seen" counts samples that read it. "Resident" is its presence in the last complete main listing, at 07:40. Rows run smallest first, the order they are admitted. "Fit": `bounded` fits against the upper bound on preserved occupancy; `provisional` fits only against the lower bound, so it is a ceiling and not a demonstrated fit; "as 10 GiB" applies only under that allowance reading; `excluded` fits under neither bound.

| Identity                                                  | Producer                                              | Env      | Compressed bytes | Saves | Seen | Resident | Fit                   |
| --------------------------------------------------------- | ----------------------------------------------------- | -------- | ---------------- | ----- | ---- | -------- | --------------------- |
| `detect-changes-detect-changes`                           | ci / detect-changes                                   | 986f06eb | 81,333,794       | 2     | 3    | yes      | bounded               |
| `sfc-projection-sfc-projection`                           | ci / sfc-projection                                   | 986f06eb | 211,108,601      | 1     | 2    | no       | bounded as 10 GiB     |
| `svelte-compiler-benchmark-svelte-compiler-benchmark`     | ci / svelte-compiler-benchmark                        | 986f06eb | 227,198,208      | 1     | 2    | no       | bounded as 10 GiB     |
| `wasm-observe-test-wasm-observe-test`                     | ci / wasm-observe-test                                | 986f06eb | 264,488,471      | 2     | 7    | no       | bounded as 10 GiB     |
| `browser-host-browser-host`                               | ci / browser-host                                     | 60171389 | 272,915,014      | 1     | 5    | no       | provisional           |
| `build-native-node-build-native-node`                     | ci / build-native-node                                | 986f06eb | 274,921,357      | 2     | 7    | no       | provisional           |
| `transport-route-equivalence-transport-route-equivalence` | ci / transport-route-equivalence                      | b871065d | 285,848,275      | 1     | 3    | no       | provisional as 10 GiB |
| `build-editor-lsp-build-editor-lsp`                       | ci / build-editor-lsp                                 | 986f06eb | 287,995,842      | 1     | 2    | no       | provisional as 10 GiB |
| `bf2-authoritative-bf2-authoritative`                     | ci / bf2-authoritative                                | 986f06eb | 297,024,351      | 1     | 2    | no       | excluded              |
| `rust-observe-smoke-rust-observe`                         | ci / rust-observe                                     | 986f06eb | 299,455,006      | 2     | 3    | no       | excluded              |
| `native-observe-test-native-observe-test`                 | ci / native-observe-test                              | 986f06eb | 301,804,209      | 1     | 2    | no       | excluded              |
| `rust-test-rust-test-build`                               | ci / rust-test-build                                  | 986f06eb | 378,535,015      | 2     | 7    | no       | excluded              |
| `rust-scale-tier-rust-scale-tier`                         | nightly / rust-scale-tier                             | 986f06eb | 378,547,923      | 1     | 7    | yes      | excluded              |
| `rust-coverage-rust-coverage-lane`                        | nightly / rust-coverage-lane                          | 986f06eb | 380,676,167      | 1     | 7    | yes      | excluded              |
| `svelte-compiler-benchmark-svelte-compiler-benchmark`     | nightly / svelte-compiler-benchmark                   | 2f23cc0e | 396,826,875      | 1     | 7    | yes      | excluded              |
| `validation-probe-observe`                                | validation-probe / observe                            | 986f06eb | 405,742,125      | 1     | 2    | no       | excluded              |
| `editor-contracts-editor-contracts`                       | ci / editor-contracts                                 | 986f06eb | 419,045,086      | 1     | 1    | no       | excluded              |
| `validation-probe-probe`                                  | validation-probe / probe                              | 986f06eb | 426,691,864      | 1     | 3    | no       | excluded              |
| `wasm-build-wasm-build`                                   | ci / wasm-build                                       | 986f06eb | 436,369,782      | 2     | 7    | no       | excluded              |
| `rust-providers-rust-providers-live`                      | ci / rust-providers-live                              | 986f06eb | 454,465,983      | 1     | 1    | no       | excluded              |
| `cargo-wasm`                                              | nightly / build-and-deploy                            | —        | 493,828,431      | 1     | 7    | yes      | excluded              |
| `rust-build-configs-rust-build-configs`                   | ci / rust-build-configs                               | 986f06eb | 824,926,166      | 1     | 5    | no       | excluded              |
| `compile-contracts session`                               | ci / compiler-contracts, nightly / compiler-contracts | —        | 1,008,259,741    | 3     | 9    | yes      | excluded              |
| `compile-contracts session-observe`                       | ci / compiler-contracts, nightly / compiler-contracts | —        | 1,293,984,249    | 2     | 8    | no       | excluded              |
| `compile-contracts core`                                  | ci / compiler-contracts, nightly / compiler-contracts | —        | 1,399,883,384    | 3     | 12   | yes      | excluded              |
| `compile-contracts compiler`                              | ci / compiler-contracts, nightly / compiler-contracts | —        | 1,963,011,384    | 2     | 12   | yes      | excluded              |

The compile-contract groups alone total 5,665,138,758 bytes. Their keys carry a revision suffix, so every main refresh adds a whole new archive beside the previous one until it is evicted.

## Fit evaluation

The rule takes recorded bytes only, with no reserve and no assumed eviction. Measured identities are admitted smallest first, which retains the most identities, against a budget of the allowance minus the preserved occupancy. An identity without a recorded size cannot be admitted. Preserved occupancy is only ever sampled, and the largest sample came from a listing cut short, so two budgets follow from the 06:20 row:

- **Lower bound, 8,603,424,650 bytes.** This is what the cut-short listing attributed. Subtracting it from the allowance gives an upper bound on the space available. That is not demonstrated headroom: 1,250,122,211 bytes that the service counted at that moment were never attributed to an entry. The listing is sorted largest first, so the unread tail is small entries, which is where sccache's many small entries sit.
- **Upper bound, 9,853,546,861 bytes.** The lower bound plus the unattributed bytes. If all of them were preserved, this is the occupancy. The subset that fits against this bound held at every sampled moment.

| Allowance reading        | Bound | Allowance bytes | Budget bytes  | Admitted | Admitted bytes | Budget left | Excluded (measured) | Unmeasured |
| ------------------------ | ----- | --------------- | ------------- | -------- | -------------- | ----------- | ------------------- | ---------- |
| 10 GB, used for adoption | upper | 10,000,000,000  | 146,453,139   | 1        | 81,333,794     | 65,119,345  | 25                  | 14         |
| 10 GB, used for adoption | lower | 10,000,000,000  | 1,396,575,350 | 6        | 1,331,965,445  | 64,609,905  | 20                  | 14         |
| 10 GiB                   | upper | 10,737,418,240  | 883,871,379   | 4        | 784,129,074    | 99,742,305  | 22                  | 14         |
| 10 GiB                   | lower | 10,737,418,240  | 2,133,993,590 | 8        | 1,905,809,562  | 228,184,028 | 18                  | 14         |

`all-fit` fails under every row, since 14 identities are unmeasured and 18 or more measured identities are excluded. `partial-fit` is not established, and the report's `outcome` reads `unestablished` for that reason:

- The `lower` rows are ceilings. They admit identities that may not have fitted at the moment of the 06:20 reading, so the six (or eight) identities they name are provisional and are not a keying list.
- The `upper` rows are the only rows that held at every sampled moment, and they are small: one identity as 10 GB, four as 10 GiB. They still do not cover a peak between hourly samples. sccache entries went from 523 to more than 5,000 within four hours, so an unobserved peak could use the whole allowance. Nothing here bounds that peak, and the live limit could not be read to settle which allowance applies.
- Complete listings alone give a lower preserved peak (7,641,184,167 bytes at 05:20), but that peak is also a sample, taken before the 06:20 reading showed higher occupancy, so it is not a bound either.

Every identity marked `excluded` needs more than the budget left under the lower bound: 64,609,905 bytes as 10 GB, 228,184,028 as 10 GiB. Any later key implementation should treat even the `bounded` rows as a floor for the current exceptions, not as a ranking of which builds matter most: smallest-first deliberately favours count, and it leaves out `rust-test` and every compile-contract group. Finishing the investigation as `partial-fit` needs evidence that bounds preserved occupancy, such as complete listings bracketing the peak or the configured limit read with a token (see [Reproduce](#reproduce)), or measures with sccache bounded. Until then the result is undeclared and adoption is held.

### Not measured

No entry for these declared, automatically triggered identities was read in the window, so they have no size and do not fit by this rule: `perf-gate-cargo`, `build-build`, `build-lsp-debug-build-lsp-debug`, `corpus-sweep-dx-corpus-sweep-dx`, `external-corpus-dx-external-corpus-dx`, `js-build-test-js-build-test`, `lsp-release-<os-key>`, `meta-ui-benchmark-build`, `napi-release`, `perturbation-build`, `rust-clippy-rust-clippy`, `rust-observe-rust-observe`, `svelte-conformance-svelte-conformance`, `svelte-oracle-svelte-oracle`. Each saves only when its lane is selected and misses its key, and most of the window's samples were rate limited, so a save and its eviction could fall between samples. Absence is not a zero size.

No Gradle, Playwright or explicit pnpm-store entry was read either. The [CI comment](https://github.com/pikax/verter/blob/main/.github/workflows/ci.yml#L2326) gives about 2.5 GB for Gradle. That is not a measurement here and is not used, but a Gradle save of anything like that size would exceed the budget left above on either reading, so it must be measured when that lane next saves.

Workflows that main cannot trigger on its own, such as tag-triggered releases, are not main-saved identities and are outside this result.

## Reproduce

From the repository root, with Node and no credential:

```sh
node scripts/ci-cache-fit.mjs collect --out cache-obs.jsonl --samples 88 --interval-seconds 240 --full-every 15
node scripts/ci-cache-fit.mjs report --in cache-obs.jsonl
```

The figures will differ on a later run, because they describe a moving cache. A maintainer can pass `--token-env NAME` to raise the rate limit and read the configured limit, which settles which allowance reading applies. Run the same commands again once a consolidated identity has been saved from main.
