# C1 A6 residual 244 normalization diagnostic

## Result

**RESULT: FIX_PATH.**

The residual is conclusively attributable. The measured candidate's 11,557
`workspace.normalize_canonical_id.calls` consist of four families:

| candidate family | exact calls |
|---|---:|
| empty request-local overlay lookup, `ResolutionOverlaySnapshot::get` | 9,576 |
| first construction of the absolute-specifier geometry, `build_source_geometry` | 724 |
| first importer normalization in the request-local memo, reached from `resolve_package_id_from_node_modules_path` | 724 |
| unchanged Engine/workspace ingress | 533 |
| **total** | **11,557** |

The exact base/candidate delta algebra is:

```text
candidate empty-overlay lookup increase       +8,208  (9,576 - 1,368)
retained-frame/request-local reduction         -5,068  (1,448 - 6,516)
canonical transaction-replay reduction         -2,896  (0 - 2,896)
unchanged Engine/workspace ingress                  0  (533 - 533)
                                                ------
net candidate minus base                         +244
```

The 9,576 calls in the first row are not necessary normalization work in the
locked subject. Every lookup is against an empty immutable request-local
`ResolutionOverlaySnapshot`: its outcome is `None` for every spelling, so
canonicalization cannot affect its answer. A private emptiness fast path in the
existing request-local overlay owner removes those calls without changing the
map, cache ownership, public API, resolution result, ordered load set,
`NeedInputs` waves, consumed-observation order, fact witness, metric definition,
or any lifecycle. The predicted exact candidate count after that one branch is
`11,557 - 9,576 = 1,981`, below the locked maximum 11,313.

This is therefore not evidence that the lock is inconsistent with the ratified
subject. It is a final, narrow instance of semantically dead request-local
overlay work, inside the already-approved request-local/replay boundary.

## Subject and constraints

- Diagnostic checkout: `/Users/carlosrodrigues/Documents/dev/verter-c1`.
- Observed diagnostic HEAD before work: `3be9d43bcec1a563111f9a688b2cfb720523b478`.
- Measured production-code commit from the A6 receipt:
  `799140d8bbf5021efd9354f94254ad2f0d424a30`, tree
  `b516b209d148a31aa79a9c69ec6ec1a28bf04671`.
- `git diff 799140d8b..HEAD` over `crates/verter_semantic`,
  `crates/verter_workspace`, and the A6 harness is empty. The later HEAD changes
  documentation, not the measured code.
- Locked base: `d1f3d50a948597f036868543b9bb21acacd730ff`.
- Harness: `crates/verter_bench/examples/attribution_baseline.rs`, Git blob
  `efa9ea54a14772ecd87511d6bb07017aa33940ba`, the same base/candidate harness
  named by the corrected receipt.
- No production source was edited and no heavy suite was run. The only repository
  write made by this diagnostic is this report. Diagnostic binaries and coverage
  profiles were built under `/tmp`; the base comparison used a detached throwaway
  worktree.

The governing boundary is the one already recorded by the two A6 consults:

1. Cross-request `ResolutionSharedMemo` retention remains forbidden. No bounded
   normalization cache exists under an already-ratified retention owner.
2. Request/`ResolveFrame`-local pure derivation reuse and private canonical replay
   are permitted.
3. No semantic, output, load-set, observation-order, witness, cache, owner, metric,
   or public-API change is permitted.
4. Stage 2 requires the full `AttemptOutcome`/`NeedInputs` behavior and forbids
   reweighting or reinterpreting A6. The proposed fast path leaves that work and
   the counter definition intact; it only skips a canonicalization whose result
   cannot be observed when the lookup map is empty.

## Method and evidence strength

The shipped attribution site is singular by design:

```rust
pub fn normalize_canonical_id(value: &str) -> String {
    verter_audit::attribute_n!(NormalizeCanonicalId, value.len());
    verter_span::path::canonicalize_path(value)
}
```

Consequently the TSV counter proves the total but erases caller identity. This
diagnostic added no source instrumentation. It rebuilt the exact base and
candidate sources with LLVM source coverage in a temporary target directory,
then ran the exact in-process A6 harness corpus. The coverage build reproduced
the locked totals exactly:

| subject | exact attributed total |
|---|---:|
| base | 11,313 |
| candidate | 11,557 |

The harness performs four identical workload passes when invoked with
`--runs 1`: warm-up, the measured pass, and the two determinism passes. Source
region/function counts were therefore divided by four. The attributed total in
every individual pass is identical, and the resulting per-pass call-site counts
sum exactly to the TSV total. The receipt's independent smaller-corpus candidate
control also remains consistent:

```text
files 0/1/2/3 -> 13/364/651/938
files >= 1   -> 77 + 287 * files
files 40     -> 11,557
```

This is deterministic construction work, not timing or sampling noise.

## Exact candidate call-site composition

| exact calls | source / function | exact operation | classification |
|---:|---|---|---|
| 9,576 | `crates/verter_workspace/src/resolution_currency.rs:288-293`, `ResolutionOverlaySnapshot::get` | normalize the lookup key before `entries.get(...)` | removable on the empty-overlay arm |
| 724 | `crates/verter_semantic/src/resolver_core/resolve_frame.rs:497-500`, `build_source_geometry` | first normalization of the raw absolute specifier `/bench/types.ts` for a new frame | legitimate first use |
| 724 | `crates/verter_semantic/src/resolver_core/resolve_frame.rs:111-120`, `ResolutionStringMemo::normalize`, reached from `source_id_resolution.rs:412` | first normalization of the importer while testing whether it is inside `node_modules` | legitimate first use in an independent frame |
| 82 | `crates/verter_workspace/src/engine.rs:729-733`, `record_content_transition_at` | canonicalize content-transition key | unchanged ingress |
| 82 | `crates/verter_workspace/src/engine.rs:1447`, workspace read ingress | canonicalize requested file id | unchanged ingress |
| 82 | `crates/verter_workspace/src/engine.rs:1474`, workspace probe ingress | canonicalize requested path | unchanged ingress |
| 82 | `crates/verter_workspace/src/engine.rs:1558`, realpath request ingress | canonicalize requested path | unchanged ingress |
| 41 | `crates/verter_workspace/src/engine.rs:1560`, `resolved.map(...)` | canonicalize present realpath result | unchanged ingress |
| 82 | `crates/verter_workspace/src/engine.rs:1584`, manifest read ingress | canonicalize manifest path | unchanged ingress |
| 82 | `crates/verter_workspace/src/engine.rs:2406`, exact workspace lookup ingress | canonicalize requested id | unchanged ingress |
| **11,557** | | | |

The six 82-call Engine rows plus the one 41-call realpath-result row are the
unchanged 533-call Engine/workspace family.

### Why the 9,576 row is specifically empty-overlay work

`ResolutionOverlaySnapshot` is documented and implemented as an immutable
request-local overlay:

```rust
pub struct ResolutionOverlaySnapshot {
    entries: Arc<HashMap<String, Option<Arc<str>>>>,
}
```

`new` canonicalizes upsert and tombstone keys, while `get` currently
canonicalizes every lookup key. In this A6 harness no session overlay upsert or
tombstone exists. Coverage records:

- zero executions of both insertion-body normalizer lines at
  `resolution_currency.rs:273,279` (the two iterators are empty);
- 38,304 executions of the `get` normalization across the harness's four
  identical passes, i.e. exactly 9,576 per pass.

The empty map makes `get`'s result `None` independently of the key. Therefore
the normalizer call neither establishes a fact nor influences which fact is
loaded. It is not a witness-preserving requirement of the staged driver.

## Exact base composition and the 244 algebra

### Base call-site composition

| exact calls | base source family | base call sites |
|---:|---|---|
| 1,368 | empty request-local overlay lookup | `resolution_currency.rs:321-325`, `ResolutionOverlaySnapshot::get` |
| 6,516 | legacy live resolver path, 9 x 724 | `resolver.rs:513,1229,1265,1290,1301,1369,1405,1446,2055` |
| 2,896 | live transaction observation, 4 x 724 | `resolution_currency.rs:2741,2778,2781,2789` |
| 533 | Engine/workspace ingress | `engine.rs:725,1441,1468,1552,1553,1577,2398` |
| **11,313** | | |

The base legacy resolver rows include repeated normalization within one live
resolution. The approved candidate frame/memo corrections lawfully collapse
those nine calls to two first uses, reducing 6,516 to 1,448 (`-5,068`). The
approved canonical transaction replay eliminates the four live observation
normalizations (`-2,896`). These are exactly the two approved reduction families
relevant to this corpus.

The Stage-2 driver makes more reads through the immutable overlay wrapper while
loading and replaying missing observations. That raises the same empty-overlay
`get` call site from 1,368 to 9,576 (`+8,208`). Net:

```text
8,208 - 5,068 - 2,896 = 244
```

This explains why the candidate can have all approved provenance fixes and yet
remain 244 above the base total: the large staged-overlay increase is almost,
but not quite, offset by the request-local frame and replay reductions.

It does **not** prove the 9,576 normalizations are required. The overlay lookups
are required; canonicalizing their keys when the overlay has no entries is not.

## Minimal lawful fix

The permitted production change is limited to the existing private lookup in
`crates/verter_workspace/src/resolution_currency.rs`:

```rust
fn get(&self, canonical_id: &str) -> Option<Option<Arc<str>>> {
    if self.entries.is_empty() {
        return None;
    }
    self.entries
        .get(&normalize_canonical_id(canonical_id))
        .cloned()
}
```

The actual implementation should retain the fully-qualified normalizer spelling
used by the file. No other code change is authorized by this report.

Why this is inside the approved boundary:

- the object is explicitly immutable and request-local;
- the branch is in its current owner and adds no field, map, cache, memo,
  retention, or cross-request state;
- `get` is private, so no public API changes;
- the empty case already returns `None` for every possible normalized key;
- nonempty overlays still take the existing normalization path byte-for-byte;
- the driver still performs the same lookup, load, retry, and replay sequence.

This is smaller than introducing a canonical lookup API or changing
`WorkspaceRead`; both would exceed the permitted boundary. It also avoids any
claim that all overlay inputs are canonical: raw lookup behavior remains
unchanged whenever entries exist and spelling can affect the answer.

## Discriminating checks required before acceptance

### Focused RED/GREEN checks

1. **Empty overlay counter discriminator.** With
   `ResolutionOverlaySnapshot::default()`, use the attribution-enabled counter
   rail (or a test-only counter local to this private lookup), call `get` with a
   deliberately noncanonical path, assert `None`, and assert zero normalizer
   calls after the fix. Before the branch this must be RED with one call. Do not
   expose a production counter API merely to write this test.
2. **Nonempty raw-ingress control.** Construct one upsert using a canonical key,
   query it with a backslash/`./` spelling that requires canonicalization, assert
   the same `Some(source)` and exactly one lookup normalization. This prevents
   widening the trust boundary to arbitrary lookup inputs.
3. **Nonempty tombstone control.** The same raw/canonical spelling pair over a
   tombstone must still produce `Some(None)` through the normalizing path.
4. **Unknown noncanonical key control.** On a nonempty overlay, an absent raw key
   must still be normalized before absence is concluded. This distinguishes the
   empty-map proof from an unsafe “try raw then assume absent” shortcut.
5. **Revert control.** Removing only the empty branch must restore one call in
   check 1 while leaving checks 2-4 green.

### Locked semantic and driver checks

- Existing 24 converted cases and seven production-driver cases remain green.
- Existing request-frame and canonical replay provenance controls remain green.
- Result, ordered `LoadSet`s, `NeedInputs` wave sequence, consumed-observation
  order, fact witness, provider projection, and component-meta digest remain
  byte/structurally equal.
- Two independent frames and a basis-clear retain their required first-use
  normalizations.
- Structural inspection confirms no new map/cache/owner/retention/public API,
  no metric edit, and no load/witness/result change.

### Exact frozen A6 discrimination

Run the owner-defined frozen cell on the implementation SHA. The predicted
normalization result is 1,981, but acceptance is the measured conjunctive result:

- `workspace.normalize_canonical_id.calls <= 11,313`;
- semantic dispatch 4,216;
- semantic cold build 1,063;
- cacheable admission 1,063;
- component-meta digest `7161214711717846280`;
- every other wall/RSS/counter/oracle condition in the locked cell passes.

If the exact candidate does not remove all 9,576 calls, the focused coverage
split should be rerun before considering any further change. No cross-request
cache, generalized canonical-lookup API, load-set change, or threshold amendment
is authorized as a fallback.

## Authority action if the minimal fix is rejected

No authority amendment is required to implement or test the private empty-map
fast path above. If architecture review rules that even this private
request-local no-op elimination lies outside the already-approved boundary, the
implementation must stop. The required next act would then be an explicit
architecture/Stage-2 disposition authorizing the empty-overlay fast path (or the
maintainer/A6 lock authority amending the implementation lock under its blind
recalibration rules). Cross-request caching and scope expansion remain forbidden
either way.
