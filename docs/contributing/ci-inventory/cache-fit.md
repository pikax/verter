# Repository cache fit

Measured 2026-11-02 at about 21:11 and 21:16 UTC, on hosted runner caches for `pikax/verter`. **Outcome: undeclared. `all-fit` is ruled out; `partial-fit` is not established, and cache adoption stays held.** Occupancy and per-identity sizes are service-recorded compressed bytes.

- The repository was over the allowance when measured: the service reported 13,808,024,452 bytes in use against 10 GB (or 10 GiB), and the same figure on both samples taken five minutes apart.
- The 14 identities this listing attributed to a declared job total 7,723,467,624 bytes (7.72 GB). 25 further declared identities have no recorded entry in this listing, so `all-fit` is ruled out twice over. Absence is not a zero size.
- The listing is not a complete picture of what is stored. It returned 155 entries and 10,029,182,344 bytes against a reported 1,358 entries and 13,808,024,452 bytes, so **3,778,842,108 bytes were counted by the service and never attributed to an entry**. Preserved occupancy (everything that stays regardless of keying) is therefore bounded, not measured: between 2,305,714,720 bytes (what the listing attributed) and 6,084,556,828 bytes (that plus every unattributed byte).
- Against the upper bound on preserved occupancy, the subset that fits is the 11 Rust target identities at 10 GB (3,334,255,181 bytes, 581,187,991 left) and those plus `compile-contracts:session` at 10 GiB. Against the lower bound the same rule also admits `compile-contracts:session` and `compile-contracts:core` at 10 GB, and `compile-contracts:compiler` at 10 GiB, but that is a ceiling on the space available, not headroom, so those identities are **provisional**.
- This is a single moment. No subset is certified: occupancy moves with every save. The unrecoverable [earlier observations](#earlier-observations) are context only.
- Of the declared families, the largest consumer is not a target cache. The compile-contract groups alone hold 4,389,212,443 bytes in three entries.

This measures the current job-scoped keys on the standard hosted runners. It changes no key, runner, permission or retention setting, and it buys no capacity or storage. A consolidated per-profile key is not one of these rows: its archive has no recorded size until it is built and saved, so an adopted shared identity must be measured again rather than estimated from these rows.

## Method and access

[`scripts/ci-cache-fit.mjs`](https://github.com/pikax/verter/blob/main/scripts/ci-cache-fit.mjs) issues only GET requests to [GitHub's cache REST API](https://docs.github.com/en/rest/actions/cache) and classifies each key by consumer. Its `report` command derives the declared identities from `.github/workflows` rather than from a list kept here. `size_in_bytes` is the stored archive size, not an uncompressed target directory.

This repository is public, so the listing and usage endpoints answered without a credential. The worker's `GH_TOKEN` was refused with HTTP 401, so the collection ran anonymously. Other read access was refused:

| Read-only request                                             | Result                        | Consequence                                                                         |
| ------------------------------------------------------------- | ----------------------------- | ----------------------------------------------------------------------------------- |
| `actions/cache/usage`, anonymous                              | 200                           | Repository bytes and count                                                          |
| `actions/caches`, anonymous                                   | 200, 60 requests an hour      | Entry key, ref, size and times, but only part of what the service holds (see above) |
| `actions/cache/storage-limit`, anonymous                      | 401                           | **The configured limit is unavailable.** Both readings of 10 GB are evaluated below |
| `actions/jobs/<id>/logs`, `actions/runs/<id>/logs`, anonymous | 403, `Must have admin rights` | A save's log line cannot size an entry that eviction already removed                |

The collection took two repository listings, five minutes apart, with no failed request. Both reported `complete`, and both were identical: 155 entries, 10,029,182,344 listed bytes. A listing can read as complete while the bytes it attributes fall short of the reported usage, so a complete listing short of the usage is bounded the same way and never certifies a fit. No main-ref-only listing was taken, so per-ref figures below come from the repository listing's `ref` field.

The observations behind every figure here (the two raw listings and the `report` output derived from them) were retained with this change's recorded checks rather than committed, because raw machine-bound logs stay out of the tracked tree. Recompute every table from the retained observation file with the `report` command under [Reproduce](#reproduce).

## Occupancy

"Preserved" is everything that stays regardless of keying: sccache, the VS Code host, package-manager caches, and any candidate saved on a non-main ref. The lower figure is what the listing attributed to those consumers. The upper figure adds the usage the service counted but the listing never attributed.

| Quantity                                | Bytes          |
| --------------------------------------- | -------------- |
| Usage reported by the service           | 13,808,024,452 |
| Listed bytes                            | 10,029,182,344 |
| Unattributed (usage minus listed)       | 3,778,842,108  |
| Preserved bytes (lower)                 | 2,305,714,720  |
| Preserved bytes (upper)                 | 6,084,556,828  |
| Headroom as 10 GB, from reported usage  | −3,808,024,452 |
| Headroom as 10 GiB, from reported usage | −3,070,606,212 |

Occupancy by consumer, from the listing:

| Consumer             | Bytes         | Entries |
| -------------------- | ------------- | ------- |
| compile-contracts    | 4,389,212,443 | 3       |
| rust-target          | 3,334,255,181 | 11      |
| sccache              | 994,296,307   | 138     |
| node-package-manager | 983,153,278   | 2       |
| vscode-host          | 328,265,135   | 1       |

Every listed entry was saved on `refs/heads/main`. The listing attributes only 138 sccache entries and 994 MB to sccache, but sccache writes from every ref, and earlier observations (below) saw its entry count grow to more than 5,000 within hours, so the unattributed bytes are plausibly mostly sccache. That is an inference, not a measurement.

## Size per identity

Sizes are each identity's newest save on main in the listing. "Saves" counts distinct saves seen and older ones are not added to the size. All 14 were resident in the listing. Rows run smallest first, the order they are admitted. "Fit": `bounded` fits against the upper bound on preserved occupancy; `provisional` fits only against the lower bound, so it is a ceiling and not a demonstrated fit; `excluded` fits under neither bound.

| Identity                                                                                 | Compressed bytes | Saves | Fit as 10 GB | Fit as 10 GiB |
| ---------------------------------------------------------------------------------------- | ---------------- | ----- | ------------ | ------------- |
| `rust-target:sfc-projection-sfc-projection@Linux-x64#986f06eb`                           | 211,102,275      | 1     | bounded      | bounded       |
| `rust-target:wasm-observe-test-wasm-observe-test@Linux-x64#986f06eb`                     | 264,539,208      | 1     | bounded      | bounded       |
| `rust-target:browser-host-browser-host@Linux-x64#60171389`                               | 272,891,418      | 1     | bounded      | bounded       |
| `rust-target:build-native-node-build-native-node@Linux-x64#986f06eb`                     | 274,987,066      | 1     | bounded      | bounded       |
| `rust-target:transport-route-equivalence-transport-route-equivalence@Linux-x64#b871065d` | 286,015,202      | 1     | bounded      | bounded       |
| `rust-target:build-editor-lsp-build-editor-lsp@Linux-x64#986f06eb`                       | 287,989,554      | 1     | bounded      | bounded       |
| `rust-target:bf2-authoritative-bf2-authoritative@Linux-x64#986f06eb`                     | 297,002,392      | 1     | bounded      | bounded       |
| `rust-target:native-observe-test-native-observe-test@Linux-x64#986f06eb`                 | 301,838,111      | 1     | bounded      | bounded       |
| `rust-target:rust-clippy-rust-clippy@Linux-x64#986f06eb`                                 | 340,179,975      | 1     | bounded      | bounded       |
| `rust-target:rust-test-rust-test-build@Linux-x64#986f06eb`                               | 378,571,743      | 1     | bounded      | bounded       |
| `rust-target:editor-contracts-editor-contracts@Linux-x64#986f06eb`                       | 419,138,237      | 1     | bounded      | bounded       |
| `compile-contracts:session@Linux-X64`                                                    | 1,011,108,310    | 1     | provisional  | bounded       |
| `compile-contracts:core@Linux-X64`                                                       | 1,405,533,140    | 1     | provisional  | provisional   |
| `compile-contracts:compiler@Linux-X64`                                                   | 1,972,570,993    | 1     | excluded     | provisional   |

The compile-contract groups alone total 4,389,212,443 bytes. Their keys carry a revision suffix, so every main refresh adds a whole new archive beside the previous one until it is evicted.

## Fit evaluation

The rule takes recorded bytes only, with no reserve and no assumed eviction. Measured identities are admitted smallest first, which retains the most identities, against a budget of the allowance minus the preserved occupancy. An identity without a recorded size cannot be admitted. Because preserved occupancy is bracketed rather than measured, two budgets follow:

| Allowance reading        | Bound       | Budget bytes  | Admitted | Admitted bytes | Budget left   |
| ------------------------ | ----------- | ------------- | -------- | -------------- | ------------- |
| 10 GB, used for adoption | bounded     | 3,915,443,172 | 11       | 3,334,255,181  | 581,187,991   |
| 10 GB, used for adoption | provisional | 7,694,285,280 | 13       | 5,750,896,631  | 1,943,388,649 |
| 10 GiB                   | bounded     | 4,652,861,412 | 12       | 4,345,363,491  | 307,497,921   |
| 10 GiB                   | provisional | 8,431,703,520 | 14       | 7,723,467,624  | 708,235,896   |

`all-fit` fails under every row: 25 declared identities are unmeasured and `compile-contracts:compiler` is excluded as 10 GB. `partial-fit` is not established, and the report's `outcome` reads `unestablished` for that reason:

- The `provisional` rows are ceilings. They assume the unattributed bytes are not preserved, which the listing cannot show.
- The `bounded` rows held against the worst case for this one listing. They do not cover a peak between samples, nor any other moment: there are two samples, five minutes apart, with identical content.
- The service reported usage above the allowance on both samples. Eviction runs after a save, so a momentary reading can exceed the limit, but no reading of this window shows the repository under it.
- The configured limit could not be read, so which allowance applies is unsettled.

Any later key implementation should treat even the `bounded` rows as a floor for the current exceptions, not as a ranking of which builds matter most: smallest-first deliberately favours count. Finishing the investigation as `partial-fit` needs evidence that bounds preserved occupancy over time, such as complete listings bracketing the peak, the configured limit read with a token (see [Reproduce](#reproduce)), or measures with sccache bounded. Until then the result is undeclared and adoption is held.

### Not measured

No entry for these declared, automatically triggered identities was read, so they have no size and do not fit by this rule: `perf-gate-cargo`, `build-build`, `build-lsp-debug-build-lsp-debug`, `corpus-sweep-dx-corpus-sweep-dx`, `detect-changes-detect-changes`, `external-corpus-dx-external-corpus-dx`, `js-build-test-js-build-test`, `lsp-release-<os-key>`, `meta-ui-benchmark-build`, `napi-release`, `perturbation-build`, `rust-build-configs-rust-build-configs`, `rust-coverage-rust-coverage-lane`, `rust-observe-rust-observe`, `rust-observe-smoke-rust-observe`, `rust-providers-rust-providers-live`, `rust-scale-tier-rust-scale-tier`, `svelte-compiler-benchmark-svelte-compiler-benchmark`, `svelte-conformance-svelte-conformance`, `svelte-oracle-svelte-oracle`, `validation-probe-observe`, `validation-probe-probe`, `wasm-build-wasm-build`, `compile-contracts:session-observe`, and `explicit-cargo:cargo-wasm`. Each saves only when its lane is selected and misses its key, so a save can be absent from any one listing, and part of the 3.78 GB unattributed here may belong to them.

No Gradle, Playwright or explicit pnpm-store entry was read either. The [CI comment](https://github.com/pikax/verter/blob/main/.github/workflows/ci.yml#L2326) gives about 2.5 GB for Gradle. That is not a measurement here and is not used, but a Gradle save of anything like that size would exceed the budget left above on either reading, so it must be measured when that lane next saves.

Workflows that main cannot trigger on its own, such as tag-triggered releases, are not main-saved identities and are outside this result.

## Earlier observations

An earlier collection ran for about six hours on the same day with hourly samples. Its raw log was lost, so it can be neither recomputed nor verified, and nothing above depends on it. It is recorded as context only, with figures that should not be reused as evidence:

- Occupancy reached about 11.9 GB at one sample and about 9.8 GB at another, so it moved by roughly 2 GB within an hour.
- sccache entries went from about 523 to more than 5,000 within four hours, which is why the ratio of unattributed bytes to listed bytes is expected to vary.
- Several VS Code host entries were resident at once, one per ref.

## Reproduce

From the repository root, with Node and no credential:

```sh
node scripts/ci-cache-fit.mjs collect --out cache-obs.jsonl --samples 88 --interval-seconds 240 --full-every 15
node scripts/ci-cache-fit.mjs report --in cache-obs.jsonl
```

The figures will differ on a later run, because they describe a moving cache. To recompute the tables above, run `report --in` against the retained observation file rather than collecting again. A maintainer can pass `--token-env NAME` to raise the rate limit and read the configured limit, which settles which allowance reading applies. Run the same commands again once a consolidated identity has been saved from main.
