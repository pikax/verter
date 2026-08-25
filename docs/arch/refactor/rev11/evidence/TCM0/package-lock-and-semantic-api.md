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
multi-projection hover/folding/CodeLens/signature-help/formatting support). **Caveat originally recorded here, now SUPERSEDED by §3a**: static `strings` extraction on a stripped,
optimized Go binary could not isolate the literal lowercase wire method-name strings, so the type-name
evidence above was strong structural corroboration of the 4-step lifecycle rather than a byte-exact wire
trace. That caveat named its own remedy — "a live protocol capture (spawning `tsc --runExternalCode` with
a stub mapper process and tracing stdio)" — and recorded the gap as a TCM2 follow-up. **The capture has now
been done (§3a), so the delegation is withdrawn and the request half of the wire contract is closed here,
in TCM0, where the charter assigns it.**

### 3a. The wire protocol, captured live from the running compiler

`probes/probe7-mapper-wire-capture.mjs` configures a real `contentMappers` entry in a real `tsconfig.json`,
points it at a stub mapper process, runs the pinned native `tsc --project . --runExternalCode`, and records
every JSON-RPC frame TypeScript sends. Verbatim, from a green run (full transcript in
`probes/transcript.md`):

```json
--> {"jsonrpc":"2.0","id":"api1","method":"initialize","params":{"positionEncodings":["utf-8","utf-16"]}}
--> {"jsonrpc":"2.0","id":"api2","method":"openProject","params":{"configFileName":"<abs>/tsconfig.json","projectHandle":"stub-mapper@1.0.0:0","compilerOptions":{"noEmit":true,"project":"<abs>","strict":true,"configFilePath":"<abs>/tsconfig.json","runExternalCode":true}}}
--> {"jsonrpc":"2.0","id":"api3","method":"transform","params":{"fileName":"<abs>/thing.stub","content":"stub content\n","projectHandle":"stub-mapper@1.0.0:0"}}
--> {"jsonrpc":"2.0","id":"api4","method":"closeProject","params":{"projectHandle":"stub-mapper@1.0.0:0"}}
```

Facts this establishes, each asserted by the probe so a rename or reorder goes red:

| Fact | Value |
|---|---|
| Method names | **`initialize`, `openProject`, `transform`, `closeProject`** — lowercase-initial camelCase, NOT the capitalised Go type names §3 extracted |
| Lifecycle order | exactly those four, in that order, per project |
| Transport | JSON-RPC 2.0 over stdio with `Content-Length` framing |
| Request ids | strings, `api1`, `api2`, … monotonically per connection |
| `initialize` params | `{positionEncodings: ["utf-8","utf-16"]}` — the server offers BOTH and the mapper picks |
| `openProject` params | `{configFileName, projectHandle, compilerOptions}`; `runExternalCode: true` is echoed into `compilerOptions` |
| `transform` params | `{fileName, content, projectHandle}` — TypeScript supplies the file's authored bytes; the mapper is not asked to read the file itself |
| `closeProject` params | `{projectHandle}` and nothing else |
| `projectHandle` format | `{package}@{version}:{n}` |
| Dispatch scope | `transform` is sent ONLY for the mapper's declared `extensions`; the sibling `.ts` file never reaches the mapper |

**Configuration shape, also established by the capture** (each key discovered by iterating against the
binary's own typed config errors, so each is confirmed by the compiler rather than guessed): the tsconfig
key is `contentMappers`, an array whose entries carry `package` plus `extensions: string[]` (TS5024 if
absent, TS100031 if the package does not resolve through node resolution). The referenced package's
`package.json` must declare a `typescript.contentMapper` object (TS100034) whose `exec` is a non-empty
string array (TS100035). The `initialize` RESULT must carry a non-empty `diagnosticSource`; omitting it
fails the whole mapper with "The content mapper diagnostic source must not be empty". `initialize` has a
**5-second timeout** — "The content mapper did not respond to the 'initialize' request within 5 seconds"
— which is a hard design constraint on TCM2's mapper startup path.

**Independent corroboration of the acyclic invariant.** Across a complete compile, TypeScript issued
exactly four inbound frames on the mapper connection, all lifecycle requests — no query, no callback. The
probe asserts that count. This is the same conclusion §4.0a reaches structurally from the `rejectHandler`
symbols, reached here empirically from the opposite side.

**What is still NOT closed, narrowed.** The `transform` RESPONSE body shape is not captured: several
plausible encodings of the mapped output (`primary`/`outputs` carrying `extension`/`content`/`segments`)
were rejected by the decoder with "returned an output with unsupported virtual extension ''", so the exact
field layout of a SUCCESSFUL transform result remains unknown. The residual gap is therefore narrowed from
"the wire method-name spelling is unverified" (closed) to "the transform response body layout is
unverified", and is still owned by TCM2 — which will discover it directly the moment it implements a
mapper, since the decoder reports typed errors for a wrong shape.

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
OPPOSITE direction — TypeScript-as-client, Verter-as-server), the two protocols share no method names.

**Correction, 2026-08-23: this table is NOT the acyclic-invariant proof, and was previously cited as
though it were.** An earlier revision of this section, and `acyclic-invariant-test-spec.md`, rested the
invariant on "the full `APIMethodInfo` table has no content-mapper-initiated method". That argument does
not work: `APIMethodInfo` is the closed method table of the **session** API — a different protocol, in
the opposite direction — and it is simply silent about what the mapper connection can carry. Its silence
is not evidence. The genuine proof is in §4.0a.

**`ADR-021` still carries the unsound version of this argument, and this block does not fix it.** Its
ratified rejected-alternatives bullet rests the rejection on the `APIMethodInfo` table. Per
`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1 and Q8 this block is a NON-ACCEPTANCE evidence package that excludes every `ADR-021` change, so
`ADR-021` is at its ratified text and does NOT carry the §4.0a `rejectHandler` evidence. The finding
here is TCM0's; amending the ADR to rest on it is a separate ratification act belonging to the program
orchestrator and the maintainer.

### 4.0a The mapper connection's inbound direction is answered by a REJECTING handler

The real evidence is a positive structural fact in the native binary, not an absence in a different
protocol's table. Extracted from the same `lib/tsc` disassembly as §3:

```
internal/contentmapper.(*rejectHandler).HandleRequest
internal/contentmapper.(*rejectHandler).HandleNotification
internal/contentmapper.rejectHandler.HandleRequest
internal/contentmapper.rejectHandler.HandleNotification
```

JSON-RPC is bidirectional by construction, so the mapper connection necessarily HAS an inbound direction;
the question was never whether one exists, but what answers it. Upstream's answer is a type named
`rejectHandler`, installed as the connection's inbound `HandleRequest`/`HandleNotification` implementation.
The mapper cannot query TypeScript over its own connection because every inbound request and notification
on that connection is rejected by design — a deliberate upstream choice, visible in the shipped binary,
rather than an inference from a method list.

This is the concrete basis for the acyclic-invariant claim in `acyclic-invariant-test-spec.md`. It is
NOT the basis `ADR-021` cites: that document is unedited by this block (per
`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1/Q8) and still rests its rejected-alternatives bullet on the `APIMethodInfo` table. Note what the
`rejectHandler` evidence does and does not establish: it proves the mapper cannot re-enter TypeScript
*through the mapper connection*. It does not prove a Verter implementation cannot open a SEPARATE session-API
client from inside its own `transform()` handler and deadlock that way — which is precisely the cycle
TCM2's structural guard exists to prevent, and why that guard is not redundant with this finding.

### 4a. Session initialization — no hang observed under normal use

```
API constructed: 34 ms
updateSnapshot (cold, opens project): 1037 ms
```

**These are single samples, not baselines** (added 2026-08-23 after review). Ten-iteration
characterisation of the same two measurements on this host shows double-digit-or-larger spreads that
themselves drift by an order of magnitude between committed re-runs (see `performance-baselines.md`'s
addendum for the current figures — do not quote a specific multiple here, it goes stale on the next
regeneration), so neither figure is reproducible to better than an order of magnitude and neither may be
used as, or derived into, an acceptance threshold. They remain recorded here as evidence of the one thing
they genuinely establish: the cold path completes, i.e. there is no hang.
No hang in cold in-process spawn + session `initialize` + first `updateSnapshot`.

**The attach path was originally NOT tested, and is now probed — see §4a-attach.** This paragraph
previously recorded `API.fromLSPConnection` as "out of this investigation's probe budget" and an "open
verification gap for TCM3". That delegation is **withdrawn**: the probe exists
(`probes/probe8-lsp-session-attach.mjs`) and the path is exercised end to end.

### 4a-attach. `API.fromLSPConnection` — probed, and it constrains TCM3's topology choice

`probes/probe8-lsp-session-attach.mjs` spawns the pinned native `tsc --lsp -stdio`, drives a real LSP
handshake, issues `custom/initializeAPISession`, and attaches a second API client over the returned pipe.
This is the scenario closest to TCM3's "attach to the editor-owned API session" candidate.

| Result | Value |
|---|---|
| `custom/initializeAPISession` response | `{sessionId: "api-session-1", pipe: "<abs>/tsgo-api-<hex>-<hex>"}` |
| Time to that response | 2-44 ms across runs |
| **The SYNC client CANNOT attach** | throws *"Socket connections are not yet supported in the sync client"* (`dist/api/sync/client.js:11`) |
| The ASYNC client attaches | `connectViaSocket` → `createConnection(options.pipe)` (`dist/api/async/client.js:65-77`) |
| Attached client visibility | resolves the opened project and sees all 64 program files over the pipe |
| Attached client semantics | answers a real `Checker` query — resolved `interface W` and enumerated its members — not just metadata |
| **Session-attach hang** | **not observed**; attach + first snapshot completes in 91-261 ms |

**Three constraints this places on TCM3, none of which were previously recorded:**

1. **The attach topology is ASYNC-CLIENT-ONLY.** The sync client refuses socket connections outright. Any
   TCM3 design that assumed it could attach to the editor's session from the synchronous API surface is
   not implementable against this candidate. This is a capability limit, not a performance one.
2. **`fromLSPConnection` has different signatures on the two clients** — `Promise<API<true>>` on async
   (`dist/api/async/api.d.ts:47`) versus a bare `API<true>` on sync. Porting between them silently yields
   a pending Promise where an API is expected.
3. **The pipe is not guaranteed bound the instant `custom/initializeAPISession` returns.** The probe
   retries `createConnection` (bounded, 20 attempts over 2 s) because the first attempt can hit
   `ECONNREFUSED`. A TCM3 client that connects once and treats refusal as fatal will be intermittently
   broken.

**Methodological warning, recorded because it produced a false result first.** The LSP server issues its
OWN requests to the client (`client/registerCapability`). The first version of this probe never answered
them, the server blocked, and `custom/initializeAPISession` timed out after 15 s — which is
indistinguishable from the very session-attach hang the charter names. It was a harness bug, not a defect.
The probe now answers every server-initiated request and ASSERTS that it answered at least one, so a
future timeout on this path is attributable to the server rather than the harness. Any TCM3 attach
implementation must answer server-initiated requests for the same reason.

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
constraint for TCM3. Two verification gaps were originally recorded here: the exact wire method-name spelling for the
content-mapper protocol (§3) and the LSP-attach session path (§4a). **Both are now CLOSED**: the first by the live capture in §3a
(which also establishes the configuration shape and the 5-second `initialize` timeout; what remains of it
is the narrower `transform` response body layout), the second by the attach probe in §4a-attach (no hang;
and the async-only capability limit is a new, stronger finding than the gap it closes).
Per the charter's abort/rescope condition, neither gap blocks TCM0 itself (TCM0 is read-only and
authorizes no activation) — they were named here as required follow-up probes TCM2/TCM3 must close before
either can be accepted, not defects that block this investigation's own conclusions.

**Two ratified documents still recite both gaps as open, and this block edits neither.**
`rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md` clause 2 gates them to TCM2 and TCM3,
and `decisions/ADR-021-typescript-content-mapper-dual-plane.md` carries the same two carry-forward items
in its own text. `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1 and Q8 exclude every `ADR-021` change from this package, so `ADR-021` remains at its ratified text
and does not record probes 7 and 8. The probes are the evidence and a stale recital does not undo them;
reconciling either ratified document is a fresh ratification act for the program orchestrator and the
maintainer, not something this block may perform by editing the artifact that disagrees with it.

**Superseded, 2026-08-23:** the "not for production activation" half of this verdict is superseded by
`rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md` — the maintainer ruled this exact
candidate certified for production activation, having reproduced the probe result themselves ("the probe
passed previously so the mentioned version is correct"). The §4c defect and the two verification gaps
above are NOT waived by that ruling; they stay recorded exactly as written here and are carried forward
as binding TCM2/TCM3 design constraints. This paragraph records the supersession; it does not retract
the finding.

## Reproduction

```bash
curl -s https://registry.npmjs.org/typescript/7.1.0-dev.20260822.1
curl -s -o ts.tgz https://registry.npmjs.org/typescript/-/typescript-7.1.0-dev.20260822.1.tgz && tar xzf ts.tgz
curl -s -o native.tgz https://registry.npmjs.org/@typescript/typescript-darwin-arm64/-/typescript-darwin-arm64-7.1.0-dev.20260822.1.tgz && tar xzf native.tgz
strings -a package/lib/tsc | grep -oE "internal/contentmapper[/.][A-Za-z_.]*" | sort -u
npm install typescript@7.1.0-dev.20260822.1 --no-save   # in a scratch dir
```

The probe scripts themselves live in `probes/` next to this file, with their run instructions in
`probes/README.md` and the output of a full run in `probes/transcript.md`. **Correction, 2026-08-23:**
this block previously named four `.mjs` probe scripts that were never committed anywhere in the
repository — a citation to evidence that did not exist. The scripts in `probes/` are re-creations of
those four from the behaviours recorded in §4a-4c, re-executed against the same package, plus two new
ones (§6). `probes/transcript.md` is the output of that re-run, not of the original uncommitted run.

## 6. Bulk semantic-API probes — charter item 2's remaining clauses, executed

`OPEN-GAPS.md`'s `G-SEMANTIC-API-CERTIFICATION` row recorded that charter item 2 requires live probes of
"project and source-file lookup, `Program` and `TypeChecker` operations, bulk symbol/type/reference
queries, completions, diagnostics, cancellation, and failure behaviour", and that §4.0 above supplies
only an INVENTORY read out of `APIMethodInfo`'s type declaration for most of that list. Those probes have
now been executed. `probes/probe5-bulk-semantic-api.mjs` is 50+ checks across every clause; each asserts a
discriminating property rather than merely reporting a value, and the file exits non-zero if any fails.
It currently exits 0.

### 6.1 Independent re-verification of §1's package identity

Every identity claim in §1 was recomputed from a freshly downloaded tarball, not carried forward:
sha1 `c70740ef3e3d8bf9a4d3d29e38680a3f91b61a85`, sha256
`9975ea32b5ed2b46a3780693f67de1f04ca7926726081f578762d94baa5a88d2`, 476 files, `gitHead`
`d6c4afddb2c55f4a9dea7b59293a99a8fdea1799` — all four match §1 exactly. The probe harness
(`probes/harness.mjs`) additionally refuses to run against any other version, so a probe result cannot be
silently produced by a different package.

### 6.2 Findings that change TCM2/TCM3 design constraints

Five results are new, and each is a constraint rather than a curiosity.

**(a) The diagnostic wire shape is not the classic TypeScript one.** A diagnostic delivered by this API
carries `{fileName?, pos, end, code, category, text, reportsUnnecessary?, reportsDeprecated?,
messageChain?, relatedInformation?}` (`dist/api/proto.generated.d.ts:686-707`), confirmed live: the probe
asserts `pos`/`end` are numbers **and** that `start`/`length` are `undefined`, and that the message rides
`text` with `messageText` `undefined`. Any Verter-side code written against the classic
`start`/`length`/`messageText` shape reads `undefined` silently rather than failing. `pos`/`end` are
offsets into the file the diagnostic is attributed to, and `fileName` is present, so attribution needs no
caller-side bookkeeping.

**(b) There is no project-wide "find all references" primitive, and the failure mode is silent.**
`Checker.getReferencesToSymbolInFile(file, symbol)` matches only a symbol whose identity is local to that
file. Probed on a fixture where `main.ts` imports and calls `helper` from `dep.ts`: the declaration symbol
obtained from `dep.ts` finds its declaration in `dep.ts` and **zero** references in `main.ts`; the
import-site symbol in `main.ts` (`SymbolFlags.Alias`) finds two; and `getAliasedSymbol` of that alias —
i.e. the resolved declaration — again finds **zero** in `main.ts`. A cross-file references or rename
feature must therefore be assembled caller-side: enumerate candidate files, resolve each file's own local
alias symbol, and union. Passing the declaration symbol to every file returns an empty result with no
error, which is indistinguishable from "no references". This is a required design constraint for TCM3's
`References`/`Rename` capability rows, and it bears directly on the rename fail-closed rule the
Project-Bound External-TS Contract already carries.

**(c) `getCompletionsAtPosition` REJECTS any completion list that would need auto-imports.** A
member-access completion succeeds and returns exactly the member set. An identifier-position completion
in module scope throws `completion list needs auto imports` — a native-binary error string, confirmed by
`strings -a` on `lib/tsc`. Auto-import completions are therefore not obtainable from this API's
completion call at all; they must be assembled from `LanguageService.getImportEditsForSymbols` /
`getImportAdderEdits` over symbols the caller supplies. `feature-ownership-ledger.md`'s `resolve_completion`
row (#21) and the steering's "auto-imports" capability both depend on this, and neither previously
recorded it.

**(d) An out-of-range position produces a recovered Go panic, not a typed rejection.** Sending a position
past end-of-file to `getCompletionsAtPosition` panics the native server inside
`internal/ls/jsdoc_snippet.go` with `slice bounds out of range`. The IPC layer recovers it and surfaces it
to the client as an error carrying a full Go stack trace; **the session survives** — the probe asserts
both halves, re-querying the session afterwards and finding it still serving.
`probes/probe6-out-of-range-completion-panic.mjs` isolates this so it cannot contaminate probe 5's run.
The consequence for the dual-plane architecture is direct: the projection plane maps carrier positions
into generated output and the semantic plane sends those mapped positions here, so a mapping bug does not
degrade into a wrong answer — it produces a recovered panic. **TCM2/TCM3 must clamp positions on the
Verter side; validation at the callee cannot be relied on.**

**(e) Out-of-range positions on the `Checker` degrade to the file's module symbol.** `getSymbolAtPosition`
at a whitespace position, and at a position 100,000 characters past EOF, both return the **module symbol
for the file** rather than `undefined` or an error. A caller cannot distinguish "no symbol here" from
"the file itself" without checking the returned symbol's identity. Same conclusion as (d): clamp and
validate on the Verter side.

### 6.3 What the probes confirm works, so the constraints above are not read as a general verdict

Project and source-file lookup (`getProject`, `getProjects`, `getDefaultProjectForFile` — including
correctly returning `undefined` for a file outside the project — `getSourceFileNames`, `getSourceFile`
returning `undefined` for a nonexistent file, `getSourceFileMetadata`, `getCompilerOptions`,
`getConfigFileNames`, and `isSourceFileDefaultLibrary` discriminating 63 default-lib files from 3 project
files); all eight diagnostic getters, per-file, over an explicit file list in one call, and program-wide;
the `Checker`'s single-value symbol/type operations including `getDeclaredTypeOfSymbol` +
`getPropertiesOfType` enumerating an interface exactly, `getPropertyOfType`, `typeToString`,
`isTypeAssignableTo` discriminating in both directions, all ten intrinsic type accessors, `resolveName`,
and `getSymbolOfSourceFile` + `getExportsOfModule`; the **bulk array overloads** —
`getSymbolAtPosition(file, positions[])`, `getTypeAtPosition(file, positions[])`,
`getTypeOfSymbol(symbols[])`, `getSymbolOfSourceFile(files[])` — each returning exactly one entry per
input, in input order (asserted against the single-value calls), with an empty input returning `[]` rather
than an error; declaration and JavaScript emit, where `outputFiles` is a `Map` whose **key** is the file
name and whose value carries only `{text, sourceFileName}`; and fail-closed disposal on both `Snapshot`
and `Checker`.

**Cancellation, re-probed against the live objects rather than the `.d.ts`.** §4e established absence by
grepping type declarations. The probe now walks the entire prototype chain of the live `API`, `Snapshot`,
`Program`, `Checker` and `LanguageService` objects for any member matching `/cancel|abort/i` and finds
none. That is a stronger proof of the same claim, and it does not change §4e's conclusion or its
consequence for TCM3.

### 6.4 What remains open

The two gaps §5 already delegates are unchanged and are NOT closed by this section: the exact
content-mapper wire method-name spelling (TCM2) and the `API.fromLSPConnection` session-attach path
(TCM3). Both concern surfaces these probes do not touch — probes 1-6 all drive the in-process spawn path
(`new API({ cwd })`), never an attached LSP pipe, and never the content-mapper protocol, which runs in the
opposite direction (§3).

Bulk-method live correctness — the substance of `G-SEMANTIC-API-CERTIFICATION` — is EVIDENCED by §6:
the probes exist, are committed, assert discriminating properties, and were executed against the pin.

**The row itself is OPEN, not closed by this block.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1 admits these probes and their transcript as evidence but returns the round-3 candidate as wrongly
scoped and hands the incomplete contract remainder to a successor block **with fresh verification**. No
ruling decides whether charter item 2's bulk probes must be run by the block that is accepted, or whether
an amendment reallocates them — so the certification verdict is the successor's to make on independently
checkable grounds. See `OPEN-GAPS.md`'s `G-SEMANTIC-API-CERTIFICATION` row and
`successor-block-scope.md`.
