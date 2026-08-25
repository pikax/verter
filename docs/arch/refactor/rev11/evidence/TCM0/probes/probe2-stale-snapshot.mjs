// TCM0 probe 2 — cross-snapshot content currency WITHOUT fileChanges.
// This is the control that shows `updateSnapshot()` does not poll the filesystem: a new snapshot taken
// after an on-disk edit, with no `fileChanges`, is expected to still serve the pre-edit content. That is
// by-design caller-driven invalidation, NOT the defect probe 3 records.
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

section("probe2 cross-snapshot staleness WITHOUT fileChanges (control, expected by-design)");
try {
  const api = new API({ cwd: fx.root });
  const s1 = api.updateSnapshot({ openProjects: [fx.tsconfig] });
  const before = s1.getProject(fx.tsconfig).program.getSourceFile(fx.main).text.length;

  appendFileSync(fx.main, "\nexport const appended = 42;\n");

  const s2 = api.updateSnapshot({ openProjects: [fx.tsconfig] });
  const afterNoChanges = s2.getProject(fx.tsconfig).program.getSourceFile(fx.main).text.length;

  record("main.ts length before edit", before);
  record("main.ts length in new snapshot, no fileChanges passed", afterNoChanges);
  check("a new snapshot WITHOUT fileChanges does not observe the on-disk edit", () => {
    assert(
      afterNoChanges === before,
      `length changed ${before} -> ${afterNoChanges}: the server observed an edit it was never told about, so invalidation is NOT caller-driven`,
    );
    return `${afterNoChanges} chars, unchanged — updateSnapshot does not poll the filesystem`;
  });

  s2.dispose();
  s1.dispose();
  api.close();
} finally {
  fx.dispose();
}
finish();
