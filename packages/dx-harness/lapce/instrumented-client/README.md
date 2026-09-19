# WSP1L instrumented Lapce client build

The real-client automation path (charter WSP1L.1: "an instrumented client
build; record which is used and its perturbation"). The patch
`lapce-0.4.6-wsp1l.patch` adds two things to Lapce `v0.4.6`:

1. **A WSP1L drive channel** (`lapce-app/src/wsp1l.rs`): when
   `VERTER_WSP1L_DRIVE_ADDR` is set at startup, the client connects to that
   TCP address and serves newline-delimited JSON commands — `open` (path,
   1-based line/column), `type` (one keystroke), `complete`, `navigate`
   (go-to-definition), `close` (force-close the active editor tab). Each
   command is actuated **on the UI event loop** through the app's reactive
   dispatch; the ack carries exactly the UI stage stamps the loop observed.
   Without the env var the module is inert — stock runs are untouched: the
   paint hook and stage seams check one `AtomicBool` (set only once the drive
   channel connects) and never lock.
2. **UI stage stamps** — `verter ui-stamp {json}` lines on stderr, observed
   at the real pipeline seams:
   - `input_dispatched` — the drive command was accepted on the UI thread;
   - `decoded` — the payload was decoded into an editor mutation
     (`Doc::do_insert` deltas for `type`, the completion/definition response,
     the opened file content via `Doc::init_content`/`Doc::reload`);
   - `applied` — the mutation was applied to editor state (cursor + view
     deltas for `type`, filtered completion data, the dispatched jump, the
     reloaded buffer, the closed tab state);
   - `painted` — the first `EditorView` paint after `applied` (Lapce's
     invalidation and cursor-blink frames drive `EditorView` repaints).

   Stages are emitted in order per step; a stage that was not observed stays
   a hole (e.g. `close` records no `decoded`), never a guessed value.

The volt's own `verter launch-stamp` (initialize marker, behind
`uiTrace.enabled`) stays as-is; the driven capture reads both from the
client process output (`LAPCE_LOG=lapce_proxy=debug` renders the plugin
stderr through the client's tracing log).

## Build (reference machine)

```bash
curl -sL https://codeload.github.com/lapce/lapce/tar.gz/refs/tags/v0.4.6 | tar xz
cd lapce-0.4.6
git init && git add -A && git commit -m base   # so the patch applies with git apply
git apply --check <repo>/packages/dx-harness/lapce/instrumented-client/lapce-0.4.6-wsp1l.patch
git apply     <repo>/packages/dx-harness/lapce/instrumented-client/lapce-0.4.6-wsp1l.patch
cargo build --release                          # → target/release/lapce(.exe)
```

The recorded reference build reports itself as `0.4.6+Nightly.<rev>` (the
version the tarball build embeds); the capture pins that exact identity.

## Drive (record a capture)

```bash
node tests/workspace-responsiveness/WSP1L/capture-real-client.mjs \
  --lapce <built-lapce-binary> \
  --volt   <volt-dir-with volt.toml + bin/verter-lapce.wasm> \
  --server <built-verter-lsp-binary> \
  --out    tests/workspace-responsiveness/WSP1L/products/real-client-capture.v1.json
```

The runner materializes the fixture workspace
(`tests/workspace-responsiveness/WSP1L/fixtures/ws`) into a temp dir with a
`.lapce/settings.toml` pointing `lsp.serverPath` at the built server, then
`driveLapceSession` spawns the client, drives the script, and writes the
digest-sealed artifact. Perturbation recorded in every run's automation path:
the instrumentation is inert without `VERTER_WSP1L_DRIVE_ADDR`, and the
stamp emission adds one line per observed stage.
