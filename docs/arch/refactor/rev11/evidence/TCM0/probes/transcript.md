# TCM0 probe transcript

Produced by `regenerate-transcript.sh`, which runs every probe in this directory against
`typescript@7.1.0-dev.20260822.1` and redacts absolute paths (`<FIXTURE>` for the per-run OS temp
fixture root, `<ABS>` for any other absolute path). Nothing else is altered.

| field | value |
|---|---|
| date (UTC) | 2026-08-24 10:56:52 |
| host platform | Darwin 25.6.0 arm64 |
| host state | CONTENDED — other builds and agent processes were running concurrently |
| node | v20.20.2 |
| package version | 7.1.0-dev.20260822.1 |
| package gitHead | d6c4afddb2c55f4a9dea7b59293a99a8fdea1799 |

Probe 1 is measurement only and asserts nothing beyond "the cold path completes"; probes 2-8 exit
non-zero if any assertion fails. No figure in probe 1 is an acceptance threshold — see
`../performance-baselines.md`.

## `probe1-init-timing.mjs`

```

== probe1 init timing — typescript@7.1.0-dev.20260822.1 (gitHead d6c4afddb2c55f4a9dea7b59293a99a8fdea1799)
  iterations: 10
  API construction: min=1ms median=3ms max=7ms  (spread 5x)
  first updateSnapshot (cold, opens project): min=49ms median=79ms max=319ms  (spread 7x)
  second updateSnapshot (unchanged): min=0ms median=1ms max=5ms  (spread 10x)
  warm median as a fraction of cold median: 0.7%
  raw construction (ms): 7 3 3 3 3 6 3 1 5 2
  raw cold (ms): 319 179 77 88 83 69 49 69 72 81
  raw warm (ms): 1 4 5 1 1 0 0 0 0 1
  PASS  the cold session path completes on all 10 iterations (no hang) — 10/10, each opening exactly one project
  NOTE: every figure above is an observation on a shared machine, not an acceptance threshold

FAILURES: 0
exit=0
```

## `probe2-stale-snapshot.mjs`

```

== probe2 cross-snapshot staleness WITHOUT fileChanges (control, expected by-design)
  main.ts length before edit: 517
  main.ts length in new snapshot, no fileChanges passed: 517
  PASS  a new snapshot WITHOUT fileChanges does not observe the on-disk edit — 517 chars, unchanged — updateSnapshot does not poll the filesystem

FAILURES: 0
exit=0
```

## `probe3-stale-sourcefile-confirm.mjs`

```

== probe3 post-dispose Program method behaviour
  getSourceFile after dispose: returned in 0ms, identical object: true
  PASS  getSourceFile SURVIVES its snapshot's dispose and serves the cached object — identical object returned in 0ms with no server round-trip
  PASS  getSemanticDiagnostics fails closed after dispose — threw: api: client error: snapshot 1 not found
  PASS  getSourceFileNames fails closed after dispose — threw: api: client error: snapshot 1 not found
  PASS  emitToString fails closed after dispose — threw: api: client error: snapshot 1 not found
  PASS  getSyntacticDiagnostics fails closed after dispose — threw: api: client error: snapshot 1 not found
  PASS  the asymmetry is exactly one method wide — 1 of 5 probed Program methods serves stale data; the other 4 fail closed
  PASS  snapshot reports itself disposed — true

FAILURES: 0
exit=0
```

## `probe4-filechanges-correct.mjs`

```

== probe4 cross-snapshot currency WITH fileChanges.changed
  main.ts length before edit: 517
  main.ts length after edit, fileChanges.changed passed: 546
  delta observed: 29
  bytes appended: 29
  PASS  fileChanges.changed makes the next snapshot observe exactly the appended bytes — delta 29 == 29 bytes appended

FAILURES: 0
exit=0
```

## `probe5-bulk-semantic-api.mjs`

```

== probe5 bulk semantic API — typescript@7.1.0-dev.20260822.1 (gitHead d6c4afddb2c55f4a9dea7b59293a99a8fdea1799)

== 5.1 project and source-file lookup
  PASS  getProject(tsconfig) resolves — configFileName=<FIXTURE>
  PASS  getProjects() enumerates exactly the opened project — 1 project(s)
  PASS  getDefaultProjectForFile(main.ts) resolves to the configured project — id=<FIXTURE>
  PASS  getDefaultProjectForFile returns undefined for a file outside the project — undefined
  PASS  program.getSourceFileNames() — 66 file(s), includes main.ts: true
  PASS  program.getSourceFile(main.ts) returns real text — 517 chars
  PASS  program.getSourceFile returns undefined for a nonexistent file — undefined (fail-soft, no throw)
  PASS  program.getSourceFileMetadata(main.ts) carries the documented field set — keys=isDefaultLibrary,isFromExternalLibrary,packageJsonType,packageJsonDirectory,impliedNodeFormat
  PASS  program.getCompilerOptions() round-trips the fixture tsconfig — strict=true declaration=true
  PASS  program.getConfigFileNames() names the fixture tsconfig — <FIXTURE>
  PASS  isSourceFileDefaultLibrary discriminates — 63 default-lib, 3 project file(s)

== 5.2 diagnostics (every documented kind, on a file with real errors)
  PASS  getSyntacticDiagnostics(broken.ts) returns exactly 0 — 0 diagnostic(s)
  PASS  getBindDiagnostics(broken.ts) returns exactly 0 — 0 diagnostic(s)
  PASS  getSemanticDiagnostics(broken.ts) returns exactly 2 — 2 diagnostic(s) codes=[2322,2355]
  PASS  getSuggestionDiagnostics(broken.ts) returns exactly 0 — 0 diagnostic(s)
  PASS  getDeclarationDiagnostics(broken.ts) returns exactly 0 — 0 diagnostic(s)
  PASS  getSemanticDiagnostics(broken.ts) reports the seeded type error TS2322 — code=2322 category=1 text="undefined"
  PASS  getSemanticDiagnostics(main.ts) is clean — 0 diagnostics
  PASS  getSemanticDiagnostics(bulk: [main, dep, broken]) equals the sum of the per-file calls — 2 diagnostic(s) across 3 files in ONE call, matching the per-file total
  PASS  getSemanticDiagnostics() whole-program finds the seeded errors and no lib noise — 2 diagnostic(s) program-wide
  PASS  getProgramDiagnostics is empty for a well-formed project — 0 diagnostic(s)
  PASS  getGlobalDiagnostics is empty for a well-formed project — 0 diagnostic(s)
  PASS  getConfigFileParsingDiagnostics is empty for a well-formed project — 0 diagnostic(s)
  PASS  diagnostic wire shape — the field set actually delivered — keys=[category,code,end,fileName,pos,text]
  PASS  diagnostic carries a resolvable position as pos/end (NOT start/length) — pos=13 end=16 covering "bad"; classic start/length absent
  PASS  diagnostic message text rides `text`, not `messageText` — text="Type 'string' is not assignable to type 'number'." messageText=undefined
  PASS  diagnostic carries fileName, so attribution needs no caller bookkeeping — fileName=broken.ts

== 5.3 Checker single-value symbol/type operations
  PASS  checker.getSymbolAtPosition(main.ts, makeWidget decl) — name=makeWidget flags=16
  PASS  checker.getTypeAtPosition(main.ts, makeWidget) is a function type — typeToString=(id: string, size: number) => Widget
  PASS  checker.getTypeOfSymbol round-trips through typeToString — (id: string, size: number) => Widget
  PASS  checker.getDeclaredTypeOfSymbol(Widget) enumerates its members — properties=[id,nested,size]
  PASS  checker.getPropertyOfType(Widget, 'nested') then getTypeOfSymbol — nested: { deep: boolean; }
  PASS  checker.isTypeAssignableTo(string, string) === true — true
  PASS  checker.isTypeAssignableTo(string, number) === false (discriminates) — false
  PASS  checker intrinsic type accessors each return their own type — any string number boolean void undefined null never unknown bigint
  PASS  checker.resolveName resolves a project symbol, and does NOT invent one — resolved makeWidget; unknown name resolved to undefined
  PASS  checker.getSymbolOfSourceFile(dep.ts) then getExportsOfModule — exports=[Shape,helper]

== 5.4 bulk symbol/type queries (array overloads — one round trip, many results)
  PASS  checker.getSymbolAtPosition(file, positions[]) returns one entry per position — [makeWidget, Widget, helper, viaHelper]
  PASS  checker.getTypeAtPosition(file, positions[]) returns one entry per position — [(id: string, size: number) => Widget | Widget | (w: Widget) => string | string]
  PASS  checker.getTypeOfSymbol(symbols[]) bulk overload — [(id: string, size: number) => Widget | any | (w: Widget) => string | string]
  PASS  checker.getSymbolOfSourceFile(files[]) bulk overload — ["<FIXTURE>", "<FIXTURE>"]
  PASS  bulk result order matches input order (positional contract) — order preserved: [Widget, makeWidget]
  PASS  empty bulk input returns an empty array, not an error — []

== 5.5 reference queries
  PASS  same-file references: getReferencesToSymbolInFile(main.ts, makeWidget) — 4 reference handle(s)
  PASS  there is NO project-wide references primitive: the declaration symbol finds nothing in an importing file — dep.ts: 1 ref(s); main.ts: 0 ref(s) — main.ts uses helper, yet the declaration symbol matches nothing there
  PASS  cross-file references must be assembled per file via that file's own ALIAS symbol — alias symbol: 2 ref(s) in main.ts; its resolved target: 0. A project-wide "find all references" is caller-assembled, not a server primitive.
  PASS  getReferencedSymbolsForNode fails SOFT (empty) when handed a SourceFile instead of an identifier node — 0 entries, no error — an empty result here is indistinguishable from 'no references'

== 5.6 completions
  PASS  member-access completion (no auto-imports needed) SUCCEEDS and lists the member set — 3 entr(ies): [id,nested,size]
  PASS  identifier-position completion REJECTS with 'completion list needs auto imports' — threw: completion list needs auto imports
  PASS  includeSymbol attaches a Symbol handle; omitting it does not — 3/3 with the option, 0 without

== 5.7 emit and declaration output
  PASS  program.getDeclarationEmit([main.ts]) yields a .d.ts with real content — 1 file(s); main.d.ts = 456 chars; value keys=[text,sourceFileName] (name is the MAP KEY)
  PASS  program.getJavaScriptEmit([main.ts]) yields JS — 1 file(s): main.js

== 5.8 cancellation (absence, probed on the live objects rather than the .d.ts)
  PASS  no cancellation member on API / Snapshot / Program / Checker / LanguageService — none — no cancel/abort member anywhere on the live session objects

== 5.9 failure behaviour
  PASS  a whitespace position degrades to the FILE's module symbol, not undefined — returned the module symbol (name is the QUOTED module path, "\""…) — a caller cannot distinguish "no symbol here" from "the file itself"
  PASS  a beyond-EOF position ALSO degrades to the module symbol rather than failing — returned the module symbol for a position 100000 chars past EOF — NO range validation on this path
  PASS  getSemanticDiagnostics on a file not in the project fails closed — threw: api: client error: source file not found: /definitely/not/here.ts
  PASS  api.parseConfigFile on a nonexistent tsconfig fails closed — threw: api: client error: could not read file "<ABS>"

== 5.10 disposal fail-closed
  PASS  Snapshot.getProject after dispose — threw: Snapshot is disposed
  PASS  Checker.getSymbolAtPosition after its snapshot is disposed fails closed — threw: api: client error: snapshot 2 not found

FAILURES: 0
exit=0
```

## `probe6-out-of-range-completion-panic.mjs`

```

== probe6 out-of-range completion position — recovered server panic
  main.ts length: 517
  position sent: 5517
  outcome: threw: panic: runtime error: slice bounds out of range [:5517] with length 517
  recognised as an unvalidated-index panic: true
  a Go stack trace reached the client: true
  session state after the bad position: session still serving (66 files) — the panic was recovered, not fatal
  PASS  an out-of-range completion position panics the server — panic: runtime error: slice bounds out of range, with a Go stack trace on the client
  PASS  the panic is RECOVERED — the session survives it — session still serving after the bad position
  boundary: position == text length: position == length threw: completion list needs auto imports

FAILURES: 0
exit=0
```

## `probe7-mapper-wire-capture.mjs`

```

== probe7 content-mapper wire capture — typescript@7.1.0-dev.20260822.1
  tsc exit: 2
  why tsc exits non-zero: expected — the stub answers `transform` with `{}`, which is not a usable mapped output, so the compile fails AFTER the lifecycle completes
  PASS  the compile fails for the EXPECTED reason — an unusable transform result, not a protocol error — TS100025 unsupported virtual extension — the four-step lifecycle completed, the OUTPUT was rejected
  methods captured, in order: initialize -> openProject -> transform -> closeProject
  PASS  the wire lifecycle is exactly initialize -> openProject -> transform -> closeProject — initialize -> openProject -> transform -> closeProject
  PASS  method names are lowercase-initial camelCase, not the capitalised Go type names — initialize, openProject, transform, closeProject
  PASS  initialize params offer both position encodings — {"positionEncodings":["utf-8","utf-16"]}
  PASS  openProject params carry configFileName, projectHandle and compilerOptions — keys=[configFileName,projectHandle,compilerOptions] handle=stub-mapper@1.0.0:0
  PASS  transform params carry fileName, content and the SAME projectHandle — keys=[fileName,content,projectHandle]
  PASS  transform is sent ONLY for the mapper's declared extension — 1 call, for thing.stub only — main.ts never reached the mapper
  PASS  closeProject params carry the projectHandle and nothing else — keys=[projectHandle]
  PASS  projectHandle is {package}@{version}:{n} — stub-mapper@1.0.0:0
  PASS  the mapper connection carries NO inbound request from TypeScript beyond these four — 4 frames, all lifecycle
  raw frames: 
    --> {"jsonrpc":"2.0","id":"api1","method":"initialize","params":{"positionEncodings":["utf-8","utf-16"]}}
    <-- {"jsonrpc":"2.0","id":"api1","result":{"name":"stub-mapper","version":"1.0.0","diagnosticSource":"stub-mapper","positionEncoding":"utf-8","capabilities":{}}}
    --> {"jsonrpc":"2.0","id":"api2","method":"openProject","params":{"configFileName":"<FIXTURE>","projectHandle":"stub-mapper@1.0.0:0","compilerOptions":{"noEmit":true,"project":"<FIXTURE>","strict":true,"configFilePath":"<FIXTURE>","runExternalCode":true}}}
    <-- {"jsonrpc":"2.0","id":"api2","result":{}}
    --> {"jsonrpc":"2.0","id":"api3","method":"transform","params":{"fileName":"<FIXTURE>","content":"stub content\n","projectHandle":"stub-mapper@1.0.0:0"}}
    <-- {"jsonrpc":"2.0","id":"api3","result":{}}
    --> {"jsonrpc":"2.0","id":"api4","method":"closeProject","params":{"projectHandle":"stub-mapper@1.0.0:0"}}
    <-- {"jsonrpc":"2.0","id":"api4","result":{}}

FAILURES: 0
exit=0
```

## `probe8-lsp-session-attach.mjs`

```

== probe8 LSP API-session attach — typescript@7.1.0-dev.20260822.1
  LSP initialize (ms): 20
  PASS  the LSP server advertises real capabilities — positionEncoding=utf-16
  PASS  custom/initializeAPISession returns a session id and a pipe, without hanging — sessionId=api-session-1 in 2ms
  PASS  the SYNC client CANNOT attach over a pipe — it refuses by design — threw: Socket connections are not yet supported in the sync client (dist/api/sync/client.js:11)
  async fromLSPConnection + first updateSnapshot (ms): 69
  PASS  an API client ATTACHED to the LSP session resolves the project and its files — 64 files visible over the attached pipe
  PASS  the attached client answers a real semantic query, not just metadata — resolved interface W with members [id,size] over the attached pipe
  PASS  NO session-attach hang was observed on this path — session request 2ms, attach+first snapshot 69ms, both well inside the 20000ms budget
  PASS  this harness answered the server's OWN requests, so a hang here would be the server's — 1 server-initiated request(s) answered

FAILURES: 0
exit=0
```

