// TCM0 probe 6 — DEFECT REPRODUCTION, isolated so its output does not contaminate probe 5.
//
// An out-of-range `position` passed to `LanguageService.getCompletionsAtPosition` is not validated
// before the native server indexes the source text with it. The server panics (`slice bounds out of
// range`) inside `internal/ls/jsdoc_snippet.go`. The IPC layer RECOVERS that panic and surfaces it to
// the client as an error carrying a full Go stack trace; the session stays usable. This probe asserts
// both halves, because the containment is exactly what stops this from being a session-killer.
//
// It matters to the dual-plane architecture because the projection plane maps positions from the
// carrier into the generated output and the semantic plane sends those mapped positions to this API.
// An out-of-range mapped position therefore does not produce a typed rejection; it produces a
// recovered panic. TCM2/TCM3 must clamp positions on the Verter side rather than rely on validation
// at the callee.
import {
  resolveCandidate,
  loadSyncApi,
  makeFixture,
  record,
  check,
  assert,
  section,
  finish,
} from "./harness.mjs";

const candidate = resolveCandidate();
const { API } = await loadSyncApi(candidate);
const fx = makeFixture();

section("probe6 out-of-range completion position — recovered server panic");
const api = new API({ cwd: fx.root });
try {
  const snapshot = api.updateSnapshot({ openProjects: [fx.tsconfig] });
  const ls = snapshot.getProject(fx.tsconfig).languageService;

  const inRange = fx.mainText.length;
  const outOfRange = fx.mainText.length + 5000;
  record("main.ts length", inRange);
  record("position sent", outOfRange);

  let outcome,
    panicked = false,
    sawGoStack = false;
  try {
    const info = ls.getCompletionsAtPosition(fx.main, outOfRange);
    outcome = `returned ${info?.entries?.length ?? 0} entries — NO panic, defect does not reproduce`;
  } catch (err) {
    const msg = String(err.message);
    panicked = /panic: runtime error: slice bounds out of range/.test(msg);
    sawGoStack = /goroutine \d+ \[running\]/.test(msg) || /jsdoc_snippet\.go/.test(msg);
    outcome = `threw: ${msg.split("\n")[0]}`;
  }
  record("outcome", outcome);
  record("recognised as an unvalidated-index panic", panicked);
  record("a Go stack trace reached the client", sawGoStack);

  // Is the session still usable, or did the process die?
  let sessionState,
    contained = false;
  try {
    const again = snapshot.getProject(fx.tsconfig).program.getSourceFileNames();
    contained = again.length > 0;
    sessionState = `session still serving (${again.length} files) — the panic was recovered, not fatal`;
  } catch (err) {
    sessionState = `session UNUSABLE after the bad position: ${String(err.message).split("\n")[0]}`;
  }
  record("session state after the bad position", sessionState);
  check("an out-of-range completion position panics the server", () => {
    assert(panicked, `no slice-bounds panic observed; outcome was: ${outcome}`);
    assert(sawGoStack, "panicked but no Go stack trace reached the client");
    return "panic: runtime error: slice bounds out of range, with a Go stack trace on the client";
  });
  check("the panic is RECOVERED — the session survives it", () => {
    assert(contained, `the session was unusable afterwards: ${sessionState}`);
    return "session still serving after the bad position";
  });

  // Exactly-at-length is the boundary case a naive mapper is most likely to emit (end-of-file caret).
  let boundary;
  try {
    const info = ls.getCompletionsAtPosition(fx.main, inRange);
    boundary = `position == length accepted, ${info?.entries?.length ?? 0} entries`;
  } catch (err) {
    boundary = `position == length threw: ${String(err.message).split("\n")[0]}`;
  }
  record("boundary: position == text length", boundary);
} finally {
  try {
    api.close();
  } catch {}
  fx.dispose();
}
finish();
