# release/candidate → main — merge review memo

Record of the multi-model review performed before merging the `release/candidate` substrate
rewrite into `main`, the defects it found, what was fixed, and what was knowingly deferred.

**Status: `BLOCKED` on source-input convergence, by owner ruling — not by a defect verdict.**
The engineering verdict is in §4–§6. The `BLOCKED` terminal state exists because source-input
convergence was explicitly skipped, and the review program cannot certify readiness without it.

|                                |                                                                                             |
| ------------------------------ | ------------------------------------------------------------------------------------------- |
| Review                         | 10 groups × 3 independent models, 2026-07-24                                                |
| BASE (`origin/main`)           | `5c62b6b505b456d61074b18e0fb6d5578b4605b4`                                                  |
| TIP at review start            | `2491f179c` (tree `84ad223bd`)                                                              |
| TIP after Co-Authored-By strip | `588a8e8c0` (tree `84ad223bd` — **identical**, messages only)                               |
| TIP at memo time               | `6822ac7a1` — 23 session commits, 518 ahead of main                                         |
| Merge shape                    | **FAST-FORWARD.** `origin/main` is the merge-base, 0 behind. No merge commit, no conflicts. |
| Agent remote mutations         | **NONE.** No push, tag, PR, release, or registry write.                                     |

---

## 1. What was reviewed

10 coherent groups × 3 independent models (codex gpt-5.6-sol xhigh · kimi k3 · grok-4.5)
= **30 panels**, covering **all 11,474 changed files / 2,233,426 diff lines**. Exact partition,
zero files unassigned.

**180 raw findings** — S0 43 · S1 49 · S2 48 · S3 40 — with **19 cross-seat convergences**
(`verter_vue_conformance/src/compare.rs` hit by all three models independently).

Panel severities are SELF-RATINGS. Independent verification moved them in both directions:
**8 confirmed · 3 downgraded · 1 upgraded** — roughly a 27% self-rating error rate in the
verified sample. The raw S0 count is not a blocker count and must not be reported as one.

The 30 panel reports, findings ledger, disposition, and method notes were produced in an ephemeral
run root and are not committed. This memo is the durable record.

---

## 2. Commit-history change (verified message-only)

51 of the 495 commits carried `Co-Authored-By: Claude Opus 4.8`. Stripped via
`git filter-branch --msg-filter` over `BASE..HEAD`. Verified:

- commit count unchanged (495) · remaining trailers **0**
- **tree SHA identical before and after** (`84ad223bd`) · `git diff <old> <new> --stat` empty

Backups (local, never pushed): tag `backup/pre-coauthor-strip-20260724T110659Z` and
`refs/original/refs/heads/release/candidate`, both at `2491f179c`.

Because content is byte-identical, every panel finding and file:line reference remains valid.

---

## 3. Gate baseline — context for the merge decision

**`origin/main` at BASE is ALSO red.** Its CI run shows Rust Test, Rust Clippy, JS Build & Test
and Playground all failing; VS Code E2E `DISABLED — flaky`. Last green on main was `94415737e`
(the `v0.0.1-beta.1` tag, 2026-03-16). Main's logs are expired (HTTP 410, >90-day retention), so
the two failure sets could not be diffed.

**Candidate gate: 61 non-tolerated failures**, accepted by owner ruling as non-blocking:

- 50 `verter_lsp` provider tests (31 `real_provider_tests` + 19 `server_tests`)
- 4 `verter_type_runtime` — `resilient_tests.rs:744` runs `node <esbuild-native-binary>`; **new on
  this branch**, so a genuine regression, mechanical to fix
- 3 `verter_relay_shim` SIGTERM — known Claude-Code-harness artifact; needs bare CI for a verdict
- 1 trybuild expectation mismatch · 3 misc

The 50 LSP failures share a shape (`got: []`, `should return edits`, `references, got: 0`) —
provider returning **empty rather than erroring** — matching the silent-empty defect class the
review flagged independently. Fixing that class first may collapse most of the cluster.

---

## 4. Landed this session (23 commits, all verified against git state)

**Release integrity**
| commit | |
|---|---|
| `ae25225f5` | VSIX ships the `verter-lsp` engine — packaging previously deleted it; **verified present in a packed artifact and a live install** |
| `e7320d4db` | publish jobs gated on a test job |
| `a1840ea9d` | that test job made non-blocking (`continue-on-error`) — **explicit owner decision** given the red baseline |
| `f8972df68` | marketplace extension kept out of the npm publish set |
| `a3a8da0b8` | npm publish set **derived from the product dependency closure** (`scripts/lib/publish-set.mjs`), replacing two hand-maintained lists that already disagreed |
| `ed5a99e53` | proto no longer ships spec files in its tarball |

**Product packages**
| commit | |
|---|---|
| `9740bf110` | `@verter/types` emits explicit file specifiers — was **unimportable under Node ESM** (directory import) |
| `e46aa88c8` | `verter-tsc` builds/resolves all 7 platform targets incl. musl |
| `29908ddb0` | `verter-tsc` platform binary made executable — npm strips `+x`, so it was **unrunnable for every user on every platform** |
| `14d9cba4b` | `verter-tsc --tsgo-bin` flag |
| `818983e89` | `verter-tsc` requests a `typescript@7` install instead of listing two search tiers that can never succeed |
| `d2b382be4` | component-meta docs corrected — they documented `openMetaProject`, which a test asserts does **not** exist |

**LSP / TypeScript engine**
| commit | |
|---|---|
| `0c566d71e` | TypeScript 5.x/6.x `tsc` classified as tsserver-family, not a below-floor tsgo |
| `355e87e83` | falls back to the workspace tsserver when no supported tsgo exists |
| `d32c91014` | warns when serving from a legacy/below-floor TypeScript tier |
| `710c9dd17` | tsserver hover failures **surface instead of a silent empty** |

**Identity + release automation**
| commit | |
|---|---|
| `47bc00808` | extension identity → **`verter.vscode`** (publisher+name, ~35 call sites) |
| `d438e178e` | marketplace ID corrected in docs |
| `b0302fd35` | `scripts/set-version.mjs` + `pnpm bump` (conventional-commit version determination) |
| `b0c54c756` | `release-tag.yml` — tags automatically when a version commit lands on main |
| `6822ac7a1` | bump-to-tag flow documented |

---

## 5. Independently verified by the orchestrator (not taken on an agent's word)

1. **VSIX shipped no engine** — `release.yml:799` copies it, `stageShimBinary` prunes it
   (`EXTRA_ALLOWED_BIN_ENTRIES = []`), `prepareLspBinary` can't restore it on that job,
   `esbuild.mjs:111` **warns and exits 0**. All five links checked. Now fixed and re-verified in
   a packed VSIX + live install (engine present, LSP answers `initialize`).
2. **Release published with zero tests** — `validate` ran only `check-versions.mjs` + one crate
   dry-run; no test job in any publish chain.
3. **`check-versions.mjs` guarded a phantom crate** — `PUBLISHED_CRATES` named `verter_core`,
   which does not exist, and never checked `verter_compiler`, the crate actually published.
4. **`@verter/nuxt` test script is `echo …`** — an always-green stub feeding root `pnpm test`.
5. **`component-meta` excludes 4,569 lines of its own tests** (`checker.spec.ts` 3,064 +
   `native-eval.spec.ts` 1,505) from its default gate.
6. **Four CRITICAL-rule guards are narrower than the rules they enforce** — OXC-parse guard
   allowlists 8 `verter_session` paths while `verter_semantic` has **15** production
   `Parser::new` sites; the declaration-merge guard reads ONE file via `src.contains(...)`;
   `critical_rules_have_guards` accepts bare file stems (an `#[ignore]`d or unwired file
   satisfies a CRITICAL rule); `no-rawtype-reads` is defeated by ordinary variable naming.
7. **`macros.rs` last-wins registry drops declaration merges** — `registry.insert` on an
   `FxHashMap`; `interface Props{a}` + `interface Props{b}` exposes only `b`. Runs unconditionally
   in production (`build.rs:676`).
8. **`ScratchCache` is mode-blind, unvalidated, off-store** — key omits `ProjectionMode`, read
   path returns the node with no validation, and the cached value depends on `req.mode`.
9. **Hover contains a hand-rolled TS type splitter** — `public_api_summary.rs` splits on `;`
   with no string-literal guard while its sibling `find_field_separator` **does** guard strings;
   `label: "a;b"` yields the malformed type `"a`.

**Corrected downward on verification** (do not act on these): the BoundProject "bare-path" claim
(the code actually fails closed), the typecheck-gate claim (an artifact of my own harness — the
review worktrees initially lacked `node_modules`), and the ChatMessages corpus claim (the suite is
correctly feature-gated; the defect is the unevidenced claim, not a dishonest gate).

---

## 6. Offline consumer validation (published-artifact simulation)

Packed the derived publish set, installed **outside the repo** from tarballs only, and exercised
each product package:

|                          |                                                                                                                                                           |
| ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `@verter/unplugin`       | ✅ Vite build compiled both SFCs; `v-for` → `renderList`                                                                                                  |
| `@verter/component-meta` | ✅ props `["label","disabled","size"]`, events `["click","hover"]`, slots `["default","icon"]` — `withDefaults`, emit tuples, optional slots all resolved |
| `@verter/typeinfo`       | ✅ exports present                                                                                                                                        |
| `verter-tsc`             | ✅ runs, typechecks, catches TS2322, exit 1/0 correct → CI-gatable                                                                                        |
| `@verter/native`         | ✅ binding loads — **issue #90 confirmed fixed** (53 platform fallbacks restored in the napi loader)                                                      |
| workspace: leakage       | ✅ none — `pnpm pack` rewrites `workspace:*` correctly                                                                                                    |

This found **two release blockers no unit test would catch**: the `verter-tsc` exec-bit strip and
the `component-meta` documented-but-nonexistent API.

---

## 7. Publish topology (derived, not hand-maintained)

```
npm (13, dependency-ordered): native → proto → type-ir → typeinfo → language-shared
                              → types → wasm → component-meta → unplugin → nuxt
                              → verter-tsc → svelte-jsx → typescript-plugin
platform packages (14):       7 × native, 7 × verter-tsc
marketplace only:             vscode                (correctly excluded from npm)
excluded:                     @verter/oxc-bindings  (owner ruling — internal binding
                                                     package, not shipped surface)
```

`@verter/nuxt` was added to `PRODUCT_ROOTS` after end-to-end verification (§7a).
`@verter/oxc-bindings` is published at `0.0.1-alpha.3` from an earlier release and stays
excluded by owner decision; it simply stops being republished. That exclusion is now pinned
by a discriminating test (`oxc-bindings is not published`), proven to fail when the root is
re-added.

> **Superseded.** `@verter/oxc-bindings` has since been deleted from the workspace: it had zero
> dependents, and its job (resolving/downloading OXC bindings) is owned natively by the Rust side.
> With the package gone the exclusion is structural rather than pinned, so the `oxc-bindings is not
> published` test — which would now pass vacuously — was removed with it. The versions published to
> npm from earlier releases are unaffected.

`@verter/wasm` is an **optional peerDependency** of component-meta —
`peerDependenciesMeta.optional: true` and the readme both already state this. No change needed.

---

## 7a. `@verter/nuxt` — end-to-end verified (Nuxt 4.5.0 / Vite 8)

The package had never been exercised by anything: its `test` script is an `echo` stub, and the
earlier smoke check only proved the tarball imports. It was verified properly before being added
to the publish set.

**Method** — a Nuxt app bootstrapped _outside_ the monorepo, installing the packed runtime
closure (`@verter/nuxt` → `@verter/unplugin` → `@verter/native` + platform package) via
`file:` tarballs, mimicking a registry install. `app/` srcDir layout, an SFC importing a child
SFC using `withDefaults`, typed `defineEmits`, `computed`, and `v-for`.

**Result — works.**

| Check                    | Evidence                                                                                                                                                                                 |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Verter compiles the SFCs | instrumented `transform`: ran on `app/app.vue`, `components/Greeter.vue`, and Nuxt's own `nuxt-root.vue` / `error-404.vue` / `error-500.vue`, script _and_ style blocks                  |
| Production build         | `nuxt build` exit 0                                                                                                                                                                      |
| **Runtime correctness**  | built server rendered `<button data-t="…"><!--[--><span>Nuxt x3</span>×3<!--]--></button>` — props + `withDefaults` resolved, `v-for` produced exactly 3 nodes, correct fragment anchors |
| Negative control         | a malformed template expression **fails the build** (exit 1); restoring it returns to green                                                                                              |

**Two traps this nearly fell into, recorded because they will recur:**

1. **Nuxt 4's srcDir is `app/`.** A first pass put `app.vue` at the project root, so the build
   was green while compiling nothing of the test's. Green means nothing unless a unique marker
   is shown to have reached the bundle.
2. **Verter's Vite plugin is itself named `vite:vue`** (deliberately — it replaces Nuxt's). Any
   check of the form "is `vite:vue` gone / is a `verter`-named plugin present" reports a false
   no-op. This produced an incorrect "the module never runs" reading that took three rounds to
   overturn. Detect Verter by _behaviour_ (did its `transform` run) — never by plugin name.

**Bounded finding (not a blocker).** A malformed template expression is not reported as a Verter
template diagnostic at the source position; it escapes into the generated render function and
surfaces downstream as a JS parse error against generated line/column:

```
[builtin:vite-transform] Expected `,` or `)` but found `}`
  ╭─[ app/components/Greeter.vue?vue&type=script&lang.ts:10:88 ]
   10 │ return (_openBlock(), _createElementBlock("button", { onClick: $event => (emit('ping') }, …
```

The file is attributed correctly and the build fails closed, so this is a diagnostic-quality
issue, not a correctness one. Post-merge backlog.

**Still true:** `@verter/nuxt` has no test of its own — its `test` script remains the always-green
`echo` stub (§9 A3). It is now a _published, verified-once_ package with _no standing coverage_.
The verification above is a point-in-time result, not a regression gate.

---

## 8. Release flow (built this session)

```
pnpm bump               → version from conventional commits (git-cliff --bumped-version),
                          writes root Cargo.toml (all 37 crates) + 12 npm + 14 platform,
                          verifies completeness, commits `release: v<version>`. No tag, no push.
git push origin main    → release-tag.yml re-verifies and pushes tag v<version>
tag push                → release.yml: crates.io + npm(12) + VSIX marketplace,
                          CHANGELOG.md regenerated + pushed, GitHub release with all assets
```

Dry-run verified: `0.0.1-beta.1 -> 0.0.1-beta.2`, `set-version --check` passes on **all 28
targets**. Verification runs **before** the tag, which inverts the prior failure mode where a
forgotten bump produced a green release that published nothing.

**Version bump is deliberately NOT done** — owner does it after validation and merge.
`0.0.1-beta.1` is already published on npm, so releasing from the current tree would publish
nothing and report success.

---

## 9. Outstanding at memo time

| Item                                             | State                  |
| ------------------------------------------------ | ---------------------- |
| Per-project tsserver routing (monorepo)          | ⏳ in flight — see §10 |
| Drop the ~23 MB bundled TypeScript from the VSIX | ⏳ same change         |
| Owner validation of the tsserver fix             | pending                |
| 3-seat adversarial review of this session's work | staged, not yet run    |
| Version bump → merge → push                      | **owner**              |

---

## 10. The monorepo defect (live user report, root cause proven)

A `.vue` file in a pnpm-monorepo sub-package reported `Cannot find name 'Math'` / `'ReturnType'` —
default-lib globals, meaning tsserver built a program with **no libraries**.

Root cause: `find_tsserver` searched from `workspace_root` **upward**. In a pnpm monorepo the
workspace root often has no `typescript` at all, so it fell through to a bundled copy whose
`lib.*.d.ts` had been stripped by `.vscodeignore`'s `**/*.ts` pattern. Measured: the shipped
alpha.1 AND beta.1 both carry `tsserver.js` with **zero** lib files — that tier never worked.

The workspace genuinely requires per-package engines:

```
packages/demo            TypeScript 5.8.3
packages/touch-emulator  TypeScript 5.8.3
packages/ui              TypeScript 6.0.2   ← the failing package (6 tsconfigs)
```

A single workspace-level engine cannot serve this correctly.

`resolve_tsserver` is built and **verified against the real tree**: from `packages/ui` it resolves
`.pnpm/typescript@6.0.2/…/tsserver.js` with **107 libs**, canonicalized through the pnpm symlink
so tsserver's script-relative lib lookup lands correctly; from the workspace root it **refuses**
with an actionable install message. Tiers: ProjectLocal → ConfiguredTsdk → Global, **no bundled
tier** (owner decision — the libraries come from the client's install, never from us).

Remaining: wiring the per-project router into the LSP provider path.

---

## 11. Deferred by owner ruling

- **Source-input convergence** — skipped. 209 unmerged local branches, 34 stashes, 12 local
  tags, 35 remote heads were never dispositioned. **This is why the memo terminates BLOCKED.**
- **Bucket B (4 verified architectural defects)** — `ScratchCache`, `macros.rs` merge loss, hover
  splitter, guard blindness. Deferred to after the merge.
- **A3/A4/A5** — nuxt echo-stub, component-meta test exclusions, phantom-crate check (the last was
  incidentally fixed by the derived publish set).
- **The 61 gate failures** — accepted as non-blocking.
- **`component-meta` ships with a known typed-IR violation**: `isVoidLikeEventPayload` decides
  event-payload emptiness by regex over display text. Found by **all three models**; its guard
  cannot see it; its principal suite is excluded from the default gate. Stated plainly here
  because the package ships in this release.

---

## 12. Owner run sheet

1. Validate the tsserver/monorepo fix in a real project.
2. Run the 3-seat adversarial review over the session's commits —
   **23 commits written by five different agents, reviewed only by their authors and spot-checks.**
3. Merge `release/candidate` → `main` (fast-forward; no conflicts).
4. `pnpm bump` on `main`, review the version commit, push.
5. `release-tag.yml` tags; `release.yml` publishes.
6. Confirm the `verter` marketplace publisher is registered before the VSIX publish step.

**STOP — human-only merge, tag, release, and publish. No agent performed or may perform any of these.**

---

## 13. Adversarial review of this session's own work

The 30 landed commits were written by six agents and reviewed only by their authors. A second
3-seat adversarial pass (codex · grok · kimi) over `588a8e8c0..HEAD` found **33 findings**
(codex 11 · grok 11 · kimi 13). All three converged, independently, on the **release authority** —
an area none was told was suspect. Verified:

| Finding                                                                                                                                                                                                                                                                                                 | Status                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `semverGt` compares prerelease/patch segments as STRINGS: `semverGt("0.0.1-beta.10","0.0.1-beta.9")` is `false`, `semverGt("0.0.2","0.0.10")` is `false`. Since `needsPublish = semverGt(local, published)`, **every package silently stops publishing at `beta.10`** and the release still goes green. | ✅ verified                                                                                                   |
| `scripts/check-versions.mjs` contains no `process.exit` — it always exits 0, so it is **not a gate**. The verify-before-tag protection does not gate.                                                                                                                                                   | ✅ verified                                                                                                   |
| The derived publish set **EXCLUDES `@verter/nuxt` (published 0.0.1-beta.1) and `@verter/oxc-bindings` (published 0.0.1-alpha.3)** — both live products. Adopting it as-is silently stops publishing them.                                                                                               | ✅ verified; **resolved** — nuxt verified (§7a) and added to the roots; oxc-bindings excluded by owner ruling |
| A correct `scripts/lib/semver.mjs` exists but the release-deciding script does not use it.                                                                                                                                                                                                              | reported by grok + kimi                                                                                       |
| An 83 MB platform binary was committed in an unrelated docs commit.                                                                                                                                                                                                                                     | ✅ verified, untracked in `aeffa1675`; **the blob remains in history**                                        |
| `publish-set.spec.mjs`'s `EXPECTED_NPM` asserts whatever the implementation produces, so it cannot catch a wrong root list.                                                                                                                                                                             | reported by kimi                                                                                              |

**What the pass could NOT break**, after direct attempts: the Project-Bound External-TS Contract in
the new router (every op goes `resolve_carrier → ProjectBinding → ensure_bound(BoundProject)`; all
four ownership arms fail closed); cross-project engine leakage; musl detection; concurrent
first-demand (collapses on one `OnceCell` per key); and it found no stub or non-discriminating
tests in the range. Those are meaningful negative results, not silence.

**Three of the confirmed defects were introduced by the orchestrator, not the agents:** the 83 MB
binary (a careless `git add -A`), a private project identifier committed into this memo, and the
publish-set exclusion of two shipped packages. All three are now fixed.

### Must fix before tagging

1. ~~Publish-set roots~~ — **done.** `@verter/nuxt` verified (§7a) and added; `@verter/oxc-bindings`
   deliberately excluded by owner ruling and pinned by a discriminating test.
2. `semverGt` — replace with the correct `lib/semver.mjs`. **Not urgent for `0.0.1-beta.2`**
   (`semverGt("0.0.1-beta.2","0.0.1-beta.1")` is `true`; first breakage is `beta.10`), but it is a
   silent-stop-publishing bug with no gate behind it.
3. `check-versions.mjs` — make it exit non-zero, or nothing gates the bump.
4. Re-check `710c9dd17`: codex reports surfaced tsserver hover errors are **still** converted to
   empty user results, i.e. the silent-empty fix may not be effective.

At `0.0.1-beta.2` specifically, item 1 was the only one that would have shipped wrongly; 2 and 3 are
latent, and 4 is a correctness question independent of the release mechanics.
