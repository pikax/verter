// TCM0 probe 3 — the post-dispose stale `getSourceFile` asymmetry (evidence §4c).
// A `Program` handle retained past its owning `Snapshot.dispose()` keeps serving `getSourceFile` from
// the client-side cache with no validity check, while every sibling `Program` method fails closed.
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

section("probe3 post-dispose Program method behaviour");
try {
  const api = new API({ cwd: fx.root });
  const snapshot = api.updateSnapshot({ openProjects: [fx.tsconfig] });
  const program = snapshot.getProject(fx.tsconfig).program;
  const sfBefore = program.getSourceFile(fx.main);

  snapshot.dispose();

  const t0 = performance.now();
  let sfAfter, getSourceFileError;
  try {
    sfAfter = program.getSourceFile(fx.main);
  } catch (err) {
    getSourceFileError = err.message;
  }
  const elapsed = performance.now() - t0;

  record(
    "getSourceFile after dispose",
    getSourceFileError
      ? `threw: ${getSourceFileError}`
      : `returned in ${elapsed.toFixed(0)}ms, identical object: ${sfAfter === sfBefore}`,
  );

  check("getSourceFile SURVIVES its snapshot's dispose and serves the cached object", () => {
    assert(
      !getSourceFileError,
      `it threw instead: ${getSourceFileError} — the asymmetry does not reproduce`,
    );
    assert(
      sfAfter === sfBefore,
      "it returned a DIFFERENT object — not the retained client-side cache entry",
    );
    return `identical object returned in ${elapsed.toFixed(0)}ms with no server round-trip`;
  });

  const siblings = [
    "getSemanticDiagnostics",
    "getSourceFileNames",
    "emitToString",
    "getSyntacticDiagnostics",
  ];
  for (const name of siblings) {
    check(`${name} fails closed after dispose`, () => {
      try {
        program[name]();
        throw new Error("returned a value — did NOT fail closed, so the asymmetry claim is wrong");
      } catch (err) {
        assert(
          /snapshot \d+ not found/.test(err.message),
          `threw, but not the expected disposed-snapshot error: ${err.message}`,
        );
        return `threw: ${err.message}`;
      }
    });
  }

  check("the asymmetry is exactly one method wide", () => {
    assert(
      !getSourceFileError && sfAfter === sfBefore,
      "getSourceFile did not survive, so there is no asymmetry to size",
    );
    return `1 of ${siblings.length + 1} probed Program methods serves stale data; the other ${siblings.length} fail closed`;
  });

  check("snapshot reports itself disposed", () => {
    assert(snapshot.isDisposed() === true, `isDisposed() returned ${snapshot.isDisposed()}`);
    return "true";
  });
  api.close();
} finally {
  fx.dispose();
}
finish();
