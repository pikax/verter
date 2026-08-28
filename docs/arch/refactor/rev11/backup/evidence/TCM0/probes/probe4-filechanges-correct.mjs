// TCM0 probe 4 — the corrected control for probe 2: passing `fileChanges.changed` makes the new
// snapshot observe the on-disk edit. Establishes that cross-snapshot staleness is caller-driven.
import { appendFileSync } from "node:fs";
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

section("probe4 cross-snapshot currency WITH fileChanges.changed");
try {
  const api = new API({ cwd: fx.root });
  const s1 = api.updateSnapshot({ openProjects: [fx.tsconfig] });
  const before = s1.getProject(fx.tsconfig).program.getSourceFile(fx.main).text.length;

  const appended = "\nexport const appended = 42;\n";
  appendFileSync(fx.main, appended);

  const s2 = api.updateSnapshot({
    openProjects: [fx.tsconfig],
    fileChanges: { changed: [fx.main] },
  });
  const after = s2.getProject(fx.tsconfig).program.getSourceFile(fx.main).text.length;

  record("main.ts length before edit", before);
  record("main.ts length after edit, fileChanges.changed passed", after);
  record("delta observed", after - before);
  record("bytes appended", appended.length);
  check("fileChanges.changed makes the next snapshot observe exactly the appended bytes", () => {
    assert(
      after - before === appended.length,
      `observed delta ${after - before}, expected ${appended.length}`,
    );
    return `delta ${after - before} == ${appended.length} bytes appended`;
  });

  s2.dispose();
  s1.dispose();
  api.close();
} finally {
  fx.dispose();
}
finish();
