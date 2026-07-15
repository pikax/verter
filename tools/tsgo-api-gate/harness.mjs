// tsgo --api capability gate harness (the "Block 0" gate).
// Drives the USER-INSTALLED rc `typescript` tsgo sync --api client.
// Proves whether an OFF-DISK (overlay/FS-callback-only, never written to disk)
// generated TSX "carrier" can be a real member of a configured TS project.
//
// Run (from the repo root):
//   NM_BASE="$PWD" TSGO_PATH="<abs path to tsgo exe>" node tools/tsgo-api-gate/harness.mjs
// or use tools/tsgo-api-gate/run-gate.mjs which discovers TSGO_PATH for you.
//
// The fixture TS project lives in ./fixture (committed, hermetic; no node_modules —
// @spike/shared, verterjsx and the global types resolve via tsconfig paths/typeRoots).

import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
// The TS fixture project lives in ./fixture (committed, hermetic, no node_modules).
const FIXTURE = path.join(ROOT, "fixture");

// Resolve the sync API from the user-installed rc `typescript` (TS>=7)
// distribution, searching the repo node_modules (passed via NM_BASE). The
// TS7_SOURCE env names the package (`typescript` at v7); it exposes the sync
// client at the PUBLIC export subpath `<pkg>/unstable/sync`. We import through
// that public `exports` entry (not a hand-built dist/ path) so the gate exercises
// exactly the surface a real consumer uses. Portable: no junctions.
const require = createRequire(import.meta.url);
const searchPaths = (process.env.NM_BASE || "").split(path.delimiter).filter(Boolean);
const opts = { paths: searchPaths.length ? searchPaths : undefined };
const sourcePkgs = [process.env.TS7_SOURCE, "typescript"].filter(Boolean);
// Resolve the PUBLIC `<pkg>/unstable/sync` export (parameterized over the source
// package: `typescript/unstable/sync`). `require.resolve` honours the package's
// `exports` map, so this is the public entry, not an internal file path.
let syncApiSpecifier, syncApiPath;
for (const p of sourcePkgs) {
  try {
    syncApiPath = require.resolve(`${p}/unstable/sync`, opts);
    syncApiSpecifier = `${p}/unstable/sync`;
    break;
  } catch {
    /* try next */
  }
}
if (!syncApiPath)
  throw new Error(
    `could not resolve a TS>=7 sync API (<pkg>/unstable/sync) from ${JSON.stringify(sourcePkgs)}`,
  );
const { API } = await import(pathToFileURL(syncApiPath).href);
console.log(
  "resolved sync API via public export:",
  syncApiSpecifier,
  "->",
  syncApiPath.replace(/\\/g, "/"),
);
const norm = (p) => p.replace(/\\/g, "/");

const TSCONFIG = path.join(FIXTURE, "tsconfig.json");

// ---- The OFF-DISK carrier. This file is NEVER written to disk. -------------
// It lives under src/components so the tsconfig `include` glob (src/**/*.tsx)
// covers it — note: the carrier extension is .tsx, matched by the EXPLICIT
// `src/**/*.tsx` glob. An extension-specific glob does NOT auto-expand to other
// extensions, so this models a real `.vue.tsx` companion being discoverable.
// It only exists in memory and is served exclusively through the FS overlay.
const CARRIER_PATH = path.join(FIXTURE, "src", "components", "Widget.carrier.tsx");

// Imports exercise: (a) @/* path alias + baseUrl, (b) @spike/shared project
// reference / package, (c) global ambient from tsconfig `types`/`typeRoots`,
// (d) JSX under jsxImportSource. Plus ONE deliberate type error.
const CARRIER_OK = `
import { formatLabel, type FormatOptions } from "@/utils/format";
import { makeUser, type SharedUser } from "@spike/shared";

const opts: FormatOptions = { upper: true };
const label: string = formatLabel("hello", opts);

const user: SharedUser = makeUser(7);
const who: string = user.displayName;

// global from tsconfig types/typeRoots — no import:
const flag = VERTER_GLOBAL_FLAG;
const flagKind: "verter-global" = flag.kind;

// JSX under jsxImportSource "verterjsx":
const node = <div id={label}>{who}</div>;

export const widget = { label, who, node, flagKind };
export type Widget = typeof widget;
`;

// ---- The §2.9 DX proof: a plain .ts imports a BARE `./Foo.vue` specifier. -----
// This proves the REAL "import a `.vue` from plain .ts and get enhanced types" DX
// (NOT the weaker "import the carrier's own path"): the importer writes the bare
// `.vue` specifier, and the .vue→companion redirection is served PURELY by the FS
// overlay — verified mechanism (probe): tsgo's resolver appends `.tsx`/`.ts` to the
// `Foo.vue` basename and resolves `Foo.vue.tsx`, which the overlay answers. We
// therefore serve the companion at `<bare>.tsx` and serve NOTHING at the bare path.
const VUE_BARE_PATH = path.join(FIXTURE, "src", "components", "Exported.vue"); // bare specifier target; NOT served
const VUE_COMPANION_PATH = VUE_BARE_PATH + ".tsx"; // Exported.vue.tsx — served by overlay
const VUE_COMPANION_OK = `
export interface ExportedProps { label: string }
export const widget = { label: "hi" as string };
export type Widget = typeof widget;
`;
const CONSUMER_PATH = path.join(FIXTURE, "src", "components", "Consumer.ts");
const CONSUMER_OK = `
import { widget, type Widget, type ExportedProps } from "./Exported.vue";
const w: Widget = widget;
const wl: string = w.label;            // imported member type flows (string)
const p: ExportedProps = { label: wl };
export const consumed = p.label;
`;

// Same as CARRIER_OK but with ONE deliberate type error: passing a number where
// FormatOptions is required -> should produce TS2345 on the carrier, proving the
// configured Program type-checks the off-disk file.
const CARRIER_ERR = CARRIER_OK.replace(
  `const label: string = formatLabel("hello", opts);`,
  `const label: string = formatLabel("hello", 123 /* deliberate error */);`,
);

// A SECOND edit used for the incrementality test: introduce a DIFFERENT error
// (assign number to a string) so we can see the diagnostic set change.
const CARRIER_EDIT2 = CARRIER_OK.replace(
  `const who: string = user.displayName;`,
  `const who: string = user.id; /* edit2: number -> string error */`,
);

// ---- Helpers ----------------------------------------------------------------
function diagSummary(diags) {
  return diags.map((d) => ({ code: d.code, text: d.text, pos: d.pos, end: d.end }));
}
function hasCode(diags, code) {
  return diags.some((d) => d.code === code);
}
function findOffset(content, needle) {
  const i = content.indexOf(needle);
  if (i === -1) throw new Error(`needle not found: ${needle}`);
  return i;
}

const results = {};
function record(name, pass, detail) {
  results[name] = { pass, detail };
  const tag = pass ? "PASS" : "FAIL";
  console.log(`[${tag}] ${name}: ${detail}`);
}

// ---- FS overlay: serve a SET of off-disk files (carrier + optional consumer),
// fall through for everything else. `extra` maps an absolute path -> content fn.
let carrierContent = CARRIER_OK;
let readFileHits = 0;
let carrierReadHits = 0;

function makeFs(currentCarrier, extra = {}) {
  const carrierNorm = norm(CARRIER_PATH);
  const offDisk = new Map([[carrierNorm, currentCarrier]]);
  for (const [p, fn] of Object.entries(extra)) offDisk.set(norm(p), fn);
  // All off-disk files in this gate share the src/components dir.
  const sharedDirNorm = norm(path.dirname(CARRIER_PATH));
  const injectedBases = [...offDisk.keys()].map((p) => path.basename(p));
  return {
    readFile(fileName) {
      readFileHits++;
      const fn = offDisk.get(norm(fileName));
      if (fn) {
        if (norm(fileName) === carrierNorm) carrierReadHits++;
        return fn(); // string content for the off-disk file
      }
      return undefined; // fall back to the real filesystem
    },
    fileExists(fileName) {
      if (offDisk.has(norm(fileName))) return true;
      return undefined; // fall back
    },
    getAccessibleEntries(dirName) {
      // Make the off-disk files visible to directory enumeration so the `include`
      // glob (src/**/*.{ts,tsx}) discovers them as project root files.
      if (norm(dirName) === sharedDirNorm) {
        const real = (() => {
          try {
            const dirents = fs.readdirSync(dirName, { withFileTypes: true });
            return {
              files: dirents.filter((e) => e.isFile()).map((e) => e.name),
              directories: dirents.filter((e) => e.isDirectory()).map((e) => e.name),
            };
          } catch {
            return { files: [], directories: [] };
          }
        })();
        for (const b of injectedBases) if (!real.files.includes(b)) real.files.push(b);
        return real;
      }
      return undefined; // fall back
    },
  };
}

console.log("=== tsgo --api OFF-DISK CARRIER gate ===");
console.log("fixture root:", norm(FIXTURE));
console.log("carrier (never on disk):", norm(CARRIER_PATH));
console.log("on-disk carrier present?", fs.existsSync(CARRIER_PATH));
console.log("");

// ============================================================================
// PHASE 1 — off-disk carrier as a configured-project member (gate part 1 + 2)
// ============================================================================
const api = new API({
  tsserverPath: process.env.TSGO_PATH, // explicit user-installed tsgo
  cwd: FIXTURE,
  // Off-disk set: the IDE carrier, the §2.9 consumer .ts, and the .vue COMPANION
  // (served at `Exported.vue.tsx`). The BARE `Exported.vue` path is deliberately
  // NOT served — proving the redirection rides tsgo's extension-probing, not a
  // bare-path answer.
  fs: makeFs(() => carrierContent, {
    [CONSUMER_PATH]: () => CONSUMER_OK,
    [VUE_COMPANION_PATH]: () => VUE_COMPANION_OK,
  }),
});

try {
  // Sanity: the carrier file truly is not on disk.
  record(
    "carrier_is_off_disk",
    !fs.existsSync(CARRIER_PATH),
    `fs.existsSync(carrier) === ${fs.existsSync(CARRIER_PATH)} (must be false)`,
  );

  // 1a. Open the REAL configured project.
  carrierContent = CARRIER_OK;
  let snap = api.updateSnapshot({ openProject: TSCONFIG });
  const projects = snap.getProjects();
  console.log(
    "projects in snapshot:",
    projects.map((p) => norm(p.configFileName)),
  );
  const project = snap.getProject(TSCONFIG);
  record(
    "configured_project_opened",
    !!project,
    project
      ? `configFileName=${norm(project.configFileName)} compilerOptions.paths=${JSON.stringify(project.compilerOptions.paths)} jsx=${project.compilerOptions.jsx}`
      : "no project returned for tsconfig",
  );
  if (!project) throw new Error("no configured project");

  // 1b. Is the off-disk carrier associated with THIS configured project?
  const defProj = snap.getDefaultProjectForFile(CARRIER_PATH);
  record(
    "carrier_default_project_is_configured",
    !!defProj && norm(defProj.configFileName) === norm(TSCONFIG),
    defProj
      ? `getDefaultProjectForFile -> ${norm(defProj.configFileName)}`
      : "carrier has NO default project",
  );

  // 1c. Is the carrier actually IN the program's root files?
  const inRoots = project.rootFiles.some((f) => norm(f) === norm(CARRIER_PATH));
  record(
    "carrier_in_program_rootfiles",
    inRoots,
    `rootFiles includes carrier=${inRoots}; rootFiles sample=${JSON.stringify(project.rootFiles.map(norm).filter((f) => f.includes("/src/")))}`,
  );

  // 1d. Can we fetch the carrier's source file FROM the configured program?
  const sf = project.program.getSourceFile(CARRIER_PATH);
  record(
    "carrier_sourcefile_from_program",
    !!sf,
    sf
      ? `getSourceFile -> fileName=${norm(sf.fileName)} scriptKind=${sf.scriptKind}`
      : "program.getSourceFile(carrier) returned undefined",
  );

  // 1e. CLEAN carrier: semantic diagnostics. Expect NO TS2307 (module not
  // found), NO TS2304 (cannot find global), NO TS2875/JSX errors.
  const cleanDiags = project.program.getSemanticDiagnostics(CARRIER_PATH);
  console.log("clean carrier semantic diags:", JSON.stringify(diagSummary(cleanDiags), null, 2));
  const noModuleErr = !hasCode(cleanDiags, 2307);
  const noGlobalErr = !hasCode(cleanDiags, 2304);
  record(
    "carrier_no_false_ts2307",
    noModuleErr,
    noModuleErr
      ? "no TS2307 — @/* alias + @spike/shared project-ref BOTH resolved on the off-disk carrier"
      : `TS2307 present: ${JSON.stringify(diagSummary(cleanDiags.filter((d) => d.code === 2307)))}`,
  );
  record(
    "carrier_global_types_applied",
    noGlobalErr,
    noGlobalErr
      ? "no TS2304 — tsconfig types/typeRoots global VERTER_GLOBAL_FLAG in scope"
      : `TS2304 present (global NOT applied): ${JSON.stringify(diagSummary(cleanDiags.filter((d) => d.code === 2304)))}`,
  );
  record(
    "carrier_clean_is_clean",
    cleanDiags.length === 0,
    cleanDiags.length === 0
      ? "clean carrier has ZERO diagnostics under the configured program"
      : `unexpected diags: ${JSON.stringify(diagSummary(cleanDiags))}`,
  );

  // 1f. Now feed the ERROR carrier (deliberate TS2345) and re-check.
  carrierContent = CARRIER_ERR;
  snap.dispose();
  snap = api.updateSnapshot({
    openProject: TSCONFIG,
    fileChanges: { changed: [CARRIER_PATH] },
  });
  const projectErr = snap.getProject(TSCONFIG);
  const errDiags = projectErr.program.getSemanticDiagnostics(CARRIER_PATH);
  console.log("error carrier semantic diags:", JSON.stringify(diagSummary(errDiags), null, 2));
  record(
    "carrier_deliberate_error_fires",
    hasCode(errDiags, 2345),
    hasCode(errDiags, 2345)
      ? "TS2345 fired on the off-disk carrier (number not assignable to FormatOptions) — real type-check from configured Program"
      : `expected TS2345, got: ${JSON.stringify(diagSummary(errDiags))}`,
  );

  // ========================================================================
  // PHASE 2 — equivalence vs an ON-DISK twin
  // ========================================================================
  const ON_DISK = path.join(FIXTURE, "src", "components", "WidgetOnDisk.tsx");
  fs.mkdirSync(path.dirname(ON_DISK), { recursive: true });
  fs.writeFileSync(ON_DISK, CARRIER_OK, "utf8");
  try {
    // New API without the overlay carrier; on-disk twin is a normal member.
    const api2 = new API({ tsserverPath: process.env.TSGO_PATH, cwd: FIXTURE });
    const snap2 = api2.updateSnapshot({ openProject: TSCONFIG });
    const project2 = snap2.getProject(TSCONFIG);
    const onDiskDiags = project2.program.getSemanticDiagnostics(ON_DISK);
    console.log("on-disk twin diags:", JSON.stringify(diagSummary(onDiskDiags), null, 2));

    // Re-fetch off-disk clean diags for an apples-to-apples compare.
    carrierContent = CARRIER_OK;
    const snap3 = api.updateSnapshot({
      openProject: TSCONFIG,
      fileChanges: { changed: [CARRIER_PATH] },
    });
    const offDiskDiags = snap3.getProject(TSCONFIG).program.getSemanticDiagnostics(CARRIER_PATH);

    // Equivalence = IDENTICAL diagnostic codes (file-name + absolute pos differ
    // by design since the files have different names/paths). Both clean => both
    // zero; if a fixture diag exists it must appear identically on both.
    const onCodes = onDiskDiags.map((d) => d.code).sort();
    const offCodes = offDiskDiags.map((d) => d.code).sort();
    const sameClean = JSON.stringify(onCodes) === JSON.stringify(offCodes);
    record(
      "offdisk_equals_ondisk_clean",
      sameClean,
      sameClean
        ? `off-disk carrier and on-disk twin produce IDENTICAL diagnostic codes [${offCodes.join(",") || "none"}] — identical resolution under the configured program`
        : `mismatch: onDiskCodes=[${onCodes}] offDiskCodes=[${offCodes}]`,
    );
    snap2.dispose();
    snap3.dispose();
    api2.close();
  } finally {
    fs.rmSync(ON_DISK, { force: true });
  }

  // ========================================================================
  // PHASE 3 — hover + definition on the off-disk carrier (gate part 2)
  // ========================================================================
  carrierContent = CARRIER_OK;
  snap = api.updateSnapshot({ openProject: TSCONFIG, fileChanges: { changed: [CARRIER_PATH] } });
  const proj3 = snap.getProject(TSCONFIG);

  // Hover: type at the `who` identifier usage (should be string).
  const whoUseOff = findOffset(CARRIER_OK, "who, node");
  const hoverType = proj3.checker.getTypeAtPosition(CARRIER_PATH, whoUseOff);
  const hoverString = hoverType ? safeTypeToString(proj3, hoverType) : "(no type)";
  record(
    "carrier_hover_type",
    !!hoverType && hoverString === "string",
    `getTypeAtPosition(who) -> typeToString = ${JSON.stringify(hoverString)} (flags=${hoverType ? hoverType.flags : "n/a"})`,
  );

  // Definition: resolve the declaration source file(s) for a usage. The symbol
  // at an imported identifier is the import-alias (declared in the carrier); we
  // follow the alias to the underlying declaration via the value's type symbol
  // and via the symbol's own non-carrier declarations.
  const declFilesFor = (offset) => {
    const sym = proj3.checker.getSymbolAtPosition(CARRIER_PATH, offset);
    const files = new Set();
    const addDecls = (s) => {
      if (!s || !s.declarations) return;
      for (const d of s.declarations) if (d.path) files.add(norm(d.path));
    };
    addDecls(sym);
    // Follow through the type's symbol (lands on the real underlying decl).
    const t = proj3.checker.getTypeAtPosition(CARRIER_PATH, offset);
    if (t) {
      try {
        addDecls(t.getSymbol());
      } catch {}
    }
    return { sym, files: [...files] };
  };

  // formatLabel usage -> declaration in src/utils/format.ts (path alias / baseUrl)
  const { sym: flSym, files: flFiles } = declFilesFor(
    findOffset(CARRIER_OK, 'formatLabel("hello"'),
  );
  const flHit = flFiles.some((f) => f.includes("/src/utils/format.ts"));
  record(
    "carrier_definition_resolves",
    !!flSym && flHit,
    flSym
      ? `formatLabel -> ${flSym.name}; decl files=${JSON.stringify(flFiles)}`
      : "no symbol at formatLabel use",
  );

  // makeUser usage -> declaration in @spike/shared (project reference / package)
  const { sym: muSym, files: muFiles } = declFilesFor(findOffset(CARRIER_OK, "makeUser(7)"));
  const muHit = muFiles.some((f) => f.includes("@spike/shared") || f.includes("packages/shared"));
  record(
    "carrier_definition_across_projectref",
    !!muSym && muHit,
    muSym
      ? `makeUser -> ${muSym.name}; decl files=${JSON.stringify(muFiles)}`
      : "no symbol at makeUser use",
  );

  // ========================================================================
  // PHASE 4 — incremental carrier edit updates the SAME program (gate part 3)
  // ========================================================================
  // Edit the carrier content (off-disk) and push only a fileChanges delta.
  carrierContent = CARRIER_EDIT2;
  const snapEdit = api.updateSnapshot({
    openProject: TSCONFIG,
    fileChanges: { changed: [CARRIER_PATH] },
  });
  const projEdit = snapEdit.getProject(TSCONFIG);
  const editDiags = projEdit.program.getSemanticDiagnostics(CARRIER_PATH);
  console.log("edited carrier (edit2) diags:", JSON.stringify(diagSummary(editDiags), null, 2));
  // edit2 assigns user.id (number) to who: string -> TS2322.
  record(
    "carrier_edit_reflected_incrementally",
    hasCode(editDiags, 2322) && !hasCode(editDiags, 2345),
    hasCode(editDiags, 2322)
      ? `after off-disk edit + fileChanges delta, NEW diag TS2322 present and OLD TS2345 gone — same project Program updated incrementally`
      : `expected TS2322 from edit2, got: ${JSON.stringify(diagSummary(editDiags))}`,
  );

  // Now revert and confirm it goes clean again on the same project lineage.
  carrierContent = CARRIER_OK;
  const snapRevert = api.updateSnapshot({
    openProject: TSCONFIG,
    fileChanges: { changed: [CARRIER_PATH] },
  });
  const revertDiags = snapRevert.getProject(TSCONFIG).program.getSemanticDiagnostics(CARRIER_PATH);
  record(
    "carrier_edit_revert_clean",
    revertDiags.length === 0,
    revertDiags.length === 0
      ? "reverting the off-disk carrier returns zero diagnostics — clean incremental round-trip"
      : `revert diags: ${JSON.stringify(diagSummary(revertDiags))}`,
  );

  // ========================================================================
  // PHASE 5 — plain .ts imports a BARE `./Exported.vue` (§2.9 DX, real mechanism)
  // ========================================================================
  // Consumer.ts writes the BARE `.vue` specifier `import … from "./Exported.vue"`.
  // The overlay serves the COMPANION at `Exported.vue.tsx` and serves NOTHING at
  // the bare `Exported.vue` path. tsgo's resolver appends `.tsx` to the basename
  // and resolves the companion — proving the .vue→companion redirection rides the
  // FS overlay with no module-resolution-map endpoint, and that the imported
  // symbols carry the companion's real exported types. (No in-process plugin.)
  carrierContent = CARRIER_OK;
  const snapC = api.updateSnapshot({
    openProject: TSCONFIG,
    fileChanges: { changed: [CARRIER_PATH, CONSUMER_PATH, VUE_COMPANION_PATH] },
  });
  const projC = snapC.getProject(TSCONFIG);
  const consumerInRoots = projC.rootFiles.some((f) => norm(f) === norm(CONSUMER_PATH));
  const consumerDiags = projC.program.getSemanticDiagnostics(CONSUMER_PATH);
  console.log("bare-.vue consumer diags:", JSON.stringify(diagSummary(consumerDiags), null, 2));
  // Sanity: confirm the BARE path is genuinely never served by the overlay (the
  // redirection must ride extension-probing of the companion, not a bare answer).
  const bareServed = fs.existsSync(VUE_BARE_PATH);
  record(
    "bare_vue_specifier_resolves_via_overlay_redirection",
    consumerInRoots && !hasCode(consumerDiags, 2307) && consumerDiags.length === 0 && !bareServed,
    consumerInRoots
      ? consumerDiags.length === 0
        ? `plain Consumer.ts imports BARE "./Exported.vue" with ZERO diags — overlay redirected to the .vue.tsx companion (bare path on disk? ${bareServed})`
        : `consumer diags: ${JSON.stringify(diagSummary(consumerDiags))}`
      : "Consumer.ts not in program rootFiles (root-set discovery failed)",
  );
  // The imported member type flows: hover the `wl` use in `{ label: wl }` -> string.
  const wlOff = findOffset(CONSUMER_OK, "label: wl") + "label: ".length;
  const wlType = projC.checker.getTypeAtPosition(CONSUMER_PATH, wlOff);
  const wlStr = wlType ? safeTypeToString(projC, wlType) : "(no type)";
  record(
    "bare_vue_import_member_type_flows",
    !!wlType && wlStr === "string",
    `getTypeAtPosition(consumer wl) -> ${JSON.stringify(wlStr)} (the .vue companion's exported widget.label type flowed into the plain .ts importer through the bare ./Exported.vue specifier)`,
  );

  console.log(
    `\nFS overlay: total readFile calls=${readFileHits}, carrier reads=${carrierReadHits}`,
  );
} catch (e) {
  console.error("HARNESS ERROR:", e && e.stack ? e.stack : e);
  results.__harness_error = { pass: false, detail: String(e && e.message ? e.message : e) };
} finally {
  try {
    api.close();
  } catch {}
}

// ---- typeToString helper that works whether it lives on Checker or Program --
function safeTypeToString(project, type) {
  try {
    if (project.checker && typeof project.checker.typeToString === "function") {
      return project.checker.typeToString(type);
    }
  } catch (e) {
    return `(typeToString threw: ${e.message})`;
  }
  return "(no typeToString)";
}

// ---- Verdict ----------------------------------------------------------------
const order = [
  "carrier_is_off_disk",
  "configured_project_opened",
  "carrier_default_project_is_configured",
  "carrier_in_program_rootfiles",
  "carrier_sourcefile_from_program",
  "carrier_no_false_ts2307",
  "carrier_global_types_applied",
  "carrier_clean_is_clean",
  "carrier_deliberate_error_fires",
  "offdisk_equals_ondisk_clean",
  "carrier_hover_type",
  "carrier_definition_resolves",
  "carrier_definition_across_projectref",
  "carrier_edit_reflected_incrementally",
  "carrier_edit_revert_clean",
  "bare_vue_specifier_resolves_via_overlay_redirection",
  "bare_vue_import_member_type_flows",
];
console.log("\n================= VERDICT =================");
let allPass = true;
for (const k of order) {
  const r = results[k];
  if (!r) {
    console.log(`[MISS] ${k}: (not reached)`);
    allPass = false;
    continue;
  }
  if (!r.pass) allPass = false;
}
const gate1 =
  results.carrier_no_false_ts2307?.pass &&
  results.carrier_global_types_applied?.pass &&
  results.offdisk_equals_ondisk_clean?.pass;
const gate2 =
  results.carrier_deliberate_error_fires?.pass &&
  results.carrier_hover_type?.pass &&
  results.carrier_definition_resolves?.pass;
const gate3 =
  results.carrier_edit_reflected_incrementally?.pass && results.carrier_edit_revert_clean?.pass;
const gate4 =
  results.bare_vue_specifier_resolves_via_overlay_redirection?.pass &&
  results.bare_vue_import_member_type_flows?.pass;
console.log(
  `GATE 1 (resolves identically to on-disk; paths/baseUrl/types/jsx/project-refs apply): ${gate1 ? "PASS" : "FAIL"}`,
);
console.log(
  `GATE 2 (correct diagnostics + hover + definition from configured Program):           ${gate2 ? "PASS" : "FAIL"}`,
);
console.log(
  `GATE 3 (carrier-only edit updates SAME Program incrementally):                        ${gate3 ? "PASS" : "FAIL"}`,
);
console.log(
  `GATE 4 (plain .ts imports a BARE ./X.vue; overlay redirects to companion; types flow): ${gate4 ? "PASS" : "FAIL"}`,
);
console.log(`\nOVERALL: ${gate1 && gate2 && gate3 && gate4 ? "GO" : "PARTIAL/NO-GO"}`);
process.exit(gate1 && gate2 && gate3 && gate4 ? 0 : 1);
