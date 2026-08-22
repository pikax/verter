# TCM0 §1-2 — Exact package lock and semantic API certification

Scope: charter items 1 ("Exact package lock") and 2 ("Semantic API certification"). Every claim below
is against bytes actually downloaded and executed during this investigation — nothing is inferred from
the GitHub PR descriptions alone. Reproduction commands are recorded so a reviewer can re-run them.

## 1. Package identity (verified against the live registry, not documentation)

```
$ curl -s https://registry.npmjs.org/typescript/7.1.0-dev.20260822.1
```

| field | value |
|---|---|
| npm dist-tag on this version | `next` (NOT `latest` — `latest` is `7.0.2`) |
| `version` | `7.1.0-dev.20260822.1` |
| `dist.shasum` (sha1) | `c70740ef3e3d8bf9a4d3d29e38680a3f91b61a85` — verified equal to `shasum -a 1` of the downloaded tarball |
| `dist.integrity` | `sha512-SbI3P4RT3jq+Lw391NLlAo+bQFNEifaEV/c/0oPyzPXLVJJN/iH39Rx0AnSafoj3rtetzI6cyHXloMvbwt6HyA==` |
| tarball sha256 (independently computed) | `9975ea32b5ed2b46a3780693f67de1f04ca7926726081f578762d94baa5a88d2` |
| `gitHead` (source-commit provenance) | `d6c4afddb2c55f4a9dea7b59293a99a8fdea1799` in `microsoft/TypeScript` |
| unpacked size / file count | 2,903,093 bytes / 476 files |

**A package published after a merged PR does not necessarily contain every repository-main change** —
per the charter, this was checked, not assumed: the content-mapper "round 2" PR
(microsoft/TypeScript#63936) merged **2026-08-21**; the candidate is dated **2026-08-22** (one nightly
dev build later). The native binary (`@typescript/typescript-darwin-arm64@7.1.0-dev.20260822.1`,
`gitHead` also `d6c4afddb2c55f4a9dea7b59293a99a8fdea1799` — same source commit, both halves of the
split package genuinely match) was disassembled with `strings` and contains the Go symbol table for
`internal/contentmapper` — see §3 — so the merge is confirmed present in the exact candidate bytes, not
merely inferred from the date being later than the PR merge date.

Local repo state: root `package.json:98` currently pins `"typescript": "7.0.2"` (the stable `latest`,
not this candidate) everywhere except the deliberately-untouched sub-packages still on `^6.0.3`. The
candidate is not installed anywhere in the tree today — this is pure candidate discovery, per the
charter's own instruction not to treat a version string as activation authority.

## 2. What this package actually is

`typescript@7.x` is no longer a self-contained JS compiler. It is a thin JS/TS **API client** (`dist/api/**`,
`dist/ast/**`, the vendored `vscode-jsonrpc` transport) plus `bin/tsc`, which resolves and spawns a
**separate native Go binary** declared only as an `optionalDependencies` entry:

```json
"optionalDependencies": {
  "@typescript/typescript-darwin-arm64": "7.1.0-dev.20260822.1",
  "@typescript/typescript-darwin-x64": "...", "@typescript/typescript-linux-x64": "...",
  "@typescript/typescript-linux-arm": "...", "@typescript/typescript-linux-arm64": "...",
  "@typescript/typescript-win32-x64": "...", "@typescript/typescript-win32-arm64": "..."
}
```
(`getExePath.js`, resolving `@typescript/typescript-<platform>-<arch>/lib/tsc[.exe]`). There is **no**
`tsserver.js`/`typescript.js`/`services.js` bundle anywhere in this package — the entire compiler,
checker, and language service moved into the native binary (the typescript-go/"tsgo" rewrite); the npm
`typescript` package name now ships that rewrite's client, not the classic TS implementation. This
matters directly for TCM3 ("attach to the editor-owned API session" vs "direct native client" vs
"managed process") — all three topology candidates spawn or attach to the SAME native binary; there is
no longer a lighter-weight in-process JS engine option.

## 3. Content-mapper protocol — confirmed present, exact shape recorded

**Where it lives.** The literal RPC method names the discovery names (`initialize`/`openProject`/
`transform`/`closeProject`) do **not** appear in the JS API client package — that package is the
editor-facing session client (see §4), a different protocol. The content-mapper protocol is
**server-to-external-process**: the native Go binary, when started with `--runExternalCode`, is the
*client* that spawns and drives a configured external mapper process (Verter's future TCM2 role) over
JSON-RPC. Confirmed directly by disassembling the downloaded native binary
(`@typescript/typescript-darwin-arm64@7.1.0-dev.20260822.1`, `lib/tsc`, a 24,989,266-byte Mach-O
arm64 executable) with `strings -a`:

```
internal/contentmapper.InitializeError / InitializeResult
internal/contentmapper.OpenProjectParams / OpenProjectResult
internal/contentmapper.TransformParams / TransformAndParse / TransformError / TransformErrorKind
internal/contentmapper.CloseProjectParams
internal/contentmapper.decodeTransformResult / decodeMappedOutput
internal/contentmapper.handshake / handshake.func
internal/contentmapper.Mapper / mapperConn / projectEntry / projectLease
internal/contentmapper.closeOnceReadWriteCloser (Read/Write)
internal/contentmapper.DiagnosticDirectives / DiagnosticDirectivePolicy / MappedDiagnosticDirective
internal/contentmapper.PositionEncoding / OptionPathSegment / OptionDiagnostic(Result)
internal/contentmapper.SupplementalOutput
```

This directly corroborates the upstream design description (fetched live, not from memory) —
microsoft/typescript-go#4712 (merged, per its own text, as "Content Mappers" — a four-step lifecycle
**Initialize → OpenProject → Transform (repeated) → CloseProject**) and microsoft/TypeScript#63936
("Content Mappers Round 2", merged 2026-08-21, removing `protocolVersion` in favour of LSP-style
`capabilities`, making diagnostic `code` required, allowing overlapping original-text spans, and adding
multi-projection hover/folding/CodeLens/signature-help/formatting support). **Caveat recorded
honestly**: static `strings` extraction on a stripped, optimized Go binary could not isolate the literal
lowercase wire method-name strings (Go's JSON-RPC dispatch here appears to derive method identity via
reflection/struct tags rather than adjacent string literals `grep` can isolate) — the type-name evidence
above is strong structural corroboration of the 4-step lifecycle, not a byte-exact wire trace. Getting
the exact wire spelling would need either a live protocol capture (spawning `tsc --runExternalCode` with
a stub mapper process and tracing stdio) or reading the `typescript-go` Go source directly — neither was
done in this pass; recorded as an open follow-up for TCM2, not papered over.

**Trust/`--runExternalCode`** — confirmed in the JS client, `dist/api/options.d.ts:16-17`:
```ts
/** Allow trusted projects to execute configured external content mapper processes. */
runExternalCode?: boolean;
```
turned into a CLI flag (`dist/api/options.js`): `if (options.runExternalCode) args.push("--runExternalCode");`.
Matches the upstream text fetched live: *"VS Code passes `--runExternalCode` to `tsc --lsp` only in
trusted workspaces; otherwise, `contentMappers` are ignored in the LSP server."*

**Per-file identity carried on the wire once a mapper has run** — `dist/ast/ast.d.ts:70-90`
(`SourceFile` interface, quoted in full by the sub-investigation): `contentMapper?: string` ("Identity of
the content mapper that produced this source file"), `virtualFileName?: string`, `diagnosticDirectives?:
readonly MappedDiagnosticDirective[]`, `supplementalSourceFileNames?`/`canonicalSourceFileName?` (the
canonical↔supplemental pairing for e.g. multiple Astro-style script blocks from one source). These are
plain identity/metadata strings, never an RPC handle — confirming the discovery's own framing ("not a
reverse semantic-query interface").

**The span-mapping engine (`dist/ast/spanMap.d.ts`/`.js`)** is generated directly from the Go source
(`// Code generated ... from tsc/internal/spanmap/spanmap.go. DO NOT EDIT.`) — i.e. its canonical
implementation is native, this package carries only the generated mirror + a pure-JS query-side
`SpanMap` class. Three enums matter for TCM0's "projection-class contract" (§5 of the ledger doc):
- `SpanMapKind`: `Verbatim=0, Atom=1, Alias=2` — a segment's copy semantics.
- `SpanMapFidelity`: `Exact=0, Atom=1, Approximate=2, None=3` — with `SpanMap.isExact`/`isSingleSegment`/
  `isNone` static predicates.
- `SpanMapFeature`: a 20-bit bitflag (`Hover|SignatureHelp|Completion|Definition|TypeDefinition|
  Implementation|References|DocumentHighlights|Rename|CallHierarchy|CodeActions|Formatting|InlayHints|
  SemanticTokens|FoldingRanges|SelectionRanges|LinkedEditing|AutoInsert|DocumentSymbols|CodeLens|All`) —
  this is the mechanism TypeScript itself uses to decide, **per generated segment**, which IDE features
  are legal over content a content mapper produced. This is the terminal policy surface the charter's
  "projection-class contract" (item 5) must ratify Verter's own class set against — TypeScript already
  has one, and Verter's classes must compose with it rather than invent an incompatible second one.

## 4. Semantic API certification — session/snapshot lifecycle, live-probed

The npm client also exposes a **separate**, editor-facing session API (`API`/`Snapshot`/`Project`/
`Program`/`Checker`/`LanguageService` classes, `dist/api/sync/api.d.ts` and its async mirror) — this is
the "attach to the editor-owned API session" / "direct native client" topology candidate from charter
item 7, and the surface the charter's item 2 asks to be probed for the stale-snapshot and
API-session-hang defect classes. **This was executed live against the exact candidate**, not read from
docs: `npm install typescript@7.1.0-dev.20260822.1` in a scratch dir on this darwin-arm64 machine
(pulls the matching `@typescript/typescript-darwin-arm64` native binary automatically via
`optionalDependencies`), then a small Node harness exercising the real `API`/`Snapshot`/`Program`
classes against a real two-file TS project on disk.

### 4.0 The full session-API method table — confirmed one-directional, no reverse query

`APIMethodInfo` (`dist/api/proto.generated.d.ts:7-150`) is the CLOSED table of every RPC method this
session API exposes. Read in full: `release`, `initialize`, `updateSnapshot`,
`updateTemporarySnapshot`, `getDefaultProjectForFile`, `getSourceFile`, `getSourceFileMetadata`,
`getCompletionsAtPosition`, `getSyntacticDiagnostics`/`getSemanticDiagnostics`/
`getSuggestionDiagnostics`/`getDeclarationDiagnostics`, `getProgramDiagnostics`/`getGlobalDiagnostics`/
`getConfigFileParsingDiagnostics`, plus the further `Program`/`Checker`/`LanguageService` methods listed
in §7 of the sub-investigation transcript this file summarizes (symbol/type/signature queries, emit,
import-adder edits, referenced-symbols, completions). **None of these methods is a content-mapper
`Transform`/`OpenProject`/`CloseProject` call** — this table is the Verter-as-client, TypeScript-as-server
direction only. Cross-referenced against §3's `internal/contentmapper.*` Go symbol evidence (the
OPPOSITE direction — TypeScript-as-client, Verter-as-server), the two protocols share no method names and
neither table contains a call that would let one side re-enter the other mid-request. This is the
concrete basis for the acyclic-invariant claim in `acyclic-invariant-test-spec.md` and `ADR-021`'s
rejected-alternatives section: the `Transform` call (§3, confirmed via the native binary's
`TransformParams`/`TransformAndParse`/`decodeTransformResult` symbols) has no callback/query sub-protocol
back to its caller anywhere in either table.

### 4a. Session initialization — no hang observed under normal use

```
API constructed: 34 ms
updateSnapshot (cold, opens project): 1037 ms
```
No hang in cold in-process spawn + session `initialize` + first `updateSnapshot`. **Not tested**: the
`API.fromLSPConnection(...)` attach path (*"Use this when connecting to an API pipe provided by an LSP
server via `custom/initializeAPISession`"*, `dist/api/sync/api.d.ts:44-48`) — the scenario closest to
TCM3's "attach to the editor-owned API session" topology candidate. Reproducing a session-attach hang
would require spawning `tsc --lsp`, issuing `custom/initializeAPISession`, and attaching a second `API`
client to the resulting pipe; this was judged out of this investigation's probe budget and is recorded
here as an **open verification gap for TCM3**, not a passed/failed probe. Do not read the absence of a
hang in the simple case as certifying the attach path.

### 4b. Disposal is fail-closed for the `Snapshot`'s own methods

```js
snapshot.dispose();
snapshot.getProject(tsconfig);  // throws "Snapshot is disposed"
```
Confirmed clean, immediate, correctly-typed failure — `ensureNotDisposed()` (`dist/api/sync/api.js:257-259`)
guards every `Snapshot` method.

### 4c. Reproduced: a retained `Program` handle silently serves cached data after its owning snapshot is disposed

This is the closest concrete match to "the known stale-snapshot ... defect" the charter names, and it
**was reproduced against the exact candidate build**, with the client-side root cause located in source:

```js
const snapshot = api.updateSnapshot({ openProjects: [tsconfig] });
const program = snapshot.getProject(tsconfig).program;
const sfBefore = program.getSourceFile(file);      // ok
snapshot.dispose();
const sfAfter = program.getSourceFile(file);        // returns in 0ms, sfAfter === sfBefore (same JS object)
program.getSemanticDiagnostics();                   // throws "api: client error: snapshot 1 not found"
program.getSourceFileNames();                       // throws "api: client error: snapshot 1 not found"
program.emitToString();                              // throws "api: client error: snapshot 1 not found"
```

`getSourceFile` alone survives dispose, silently, with zero server round-trip (0ms) and zero error —
every other `Program` method invoked in the identical post-dispose state fails closed correctly. Root
cause, read directly from the shipped client source:

- `Program.getSourceFile` (`dist/api/sync/api.js:671-678`) checks a client-side `SourceFileCache` keyed
  by `(path, snapshotId, projectId)` **before** any server round-trip, and returns the cached entry with
  no validity check against the snapshot's disposed state.
- `Snapshot.dispose()`'s `onDispose` callback, wired at snapshot-creation time
  (`dist/api/sync/api.js:108-113`):
  ```js
  const snapshot = new Snapshot(data, this.client, this.sourceFileCache, this.toPath, () => {
      this.activeSnapshots.delete(snapshot);
      if (snapshot !== this.latestSnapshot) {
          this.sourceFileCache.releaseSnapshot(snapshot.id);
      }
  });
  ```
  **deliberately skips `releaseSnapshot` when the disposed snapshot is still `this.latestSnapshot`** —
  the cache is retained until either a newer snapshot supersedes it (which then triggers release at
  `api.js:104-106`) or `api.close()` runs (`api.js:124-126`). This is very likely intentional — it lets a
  disposed-but-still-latest snapshot's file cache warm-start the next `updateSnapshot` — but the
  **observable inconsistency** (one method silently serves stale data forever if no further snapshot is
  ever taken; every sibling method fails closed) is not documented anywhere in the shipped source as an
  intentional asymmetry, and is exactly the shape of bug a naive TCM3 caller (one that retains a
  `Program`/`Checker` past a `Snapshot.dispose()` call, which the type signatures do nothing to prevent)
  would hit silently in production.

**Corrected control, so this finding is not overclaimed:** a second probe first showed content appearing
stale even across a **brand-new** snapshot after an on-disk edit — but that was a probe-usage error, not
a defect: `updateSnapshot()` does not poll the filesystem; a caller must explicitly pass
`fileChanges: { changed: [file] } }` (`APIFileChanges`, `dist/api/proto.generated.d.ts:777-785`).
Re-run with that field set produced the correct, updated content (`106` → `127` chars, matching the
appended line). This distinction matters for TCM3: cross-snapshot staleness is by-design and
caller-driven; same-snapshot post-dispose staleness on one specific method is the genuine, narrower
defect.

**Disposition for TCM3**: any Verter-side wrapper around this `Program`/`Checker`/`Snapshot` API must
either (a) never retain a `Program`/`Checker` handle past its owning `Snapshot`'s dispose, enforced
structurally (a Rust-side type-state/ownership rule, not a runtime check), or (b) treat `getSourceFile`
results as untrustworthy once a newer snapshot could exist and always re-fetch through the *current*
`Snapshot`, never a cached `Program` reference. This is a required design constraint for TCM3, not
optional hardening.

### 4d. Not certified: the documented process-teardown deadlock avoidance

One explicit deadlock-avoidance comment exists in the shipped source (`dist/api/async/client.js:193-212`,
quoted in the sub-investigation): closing the child process is done by ending its stdin (unblocking its
read loop) rather than sending `SIGTERM`, because "Node won't exit while child is alive... Child can't
process SIGTERM while blocked on read... Read won't error until stdin is closed." This is a documented,
deliberate avoidance already in the code, not an open defect — recorded for completeness since it is the
only explicit "deadlock" text found anywhere in `dist/api/**`.

### 4e. Cancellation: absent

Grepped exhaustively across `dist/api/sync/api.d.ts`, `dist/api/async/api.d.ts`, and both proto files:
**zero** hits for `cancel`/`Cancel`/`AbortSignal`/cancellation token. The async API differs from the
sync API purely by wrapping every return type in `Promise<T>` — no cancellation parameter is added
anywhere. Any TCM3 design that assumes mid-flight cancellation of a semantic-plane query is not
supported by this candidate's API surface and must be designed around (e.g. abandon-in-place plus a
fresh snapshot, not a server-side cancel).

## 5. Certification verdict

**Certified for candidate-discovery purposes, not for production activation.** The content-mapper
protocol genuinely exists in this exact build (§3) and the basic session lifecycle behaves correctly
under normal use (§4a-b). One narrow, reproducible defect is recorded (§4c) with a required design
constraint for TCM3. Two verification gaps are recorded honestly rather than papered over: the exact
wire method-name spelling for the content-mapper protocol (§3) and the LSP-attach session path (§4a).
Per the charter's abort/rescope condition, neither gap blocks TCM0 itself (TCM0 is read-only and
authorizes no activation) — they are named here as required follow-up probes TCM2/TCM3 must close before
either can be accepted, not defects that block this investigation's own conclusions.

## Reproduction

```bash
curl -s https://registry.npmjs.org/typescript/7.1.0-dev.20260822.1
curl -s -o ts.tgz https://registry.npmjs.org/typescript/-/typescript-7.1.0-dev.20260822.1.tgz && tar xzf ts.tgz
curl -s -o native.tgz https://registry.npmjs.org/@typescript/typescript-darwin-arm64/-/typescript-darwin-arm64-7.1.0-dev.20260822.1.tgz && tar xzf native.tgz
strings -a package/lib/tsc | grep -oE "internal/contentmapper[/.][A-Za-z_.]*" | sort -u
npm install typescript@7.1.0-dev.20260822.1 --no-save   # in a scratch dir; then run the probe scripts
                                                          # (probe1-init-timing.mjs, probe2-stale-snapshot.mjs,
                                                          #  probe3-stale-sourcefile-confirm.mjs, probe4-filechanges-correct.mjs)
```
