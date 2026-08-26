// TCM0 probe 5 — the bulk semantic-API surface charter item 2 requires and the earlier evidence pass
// only inventoried: project and source-file lookup, `Program` and `Checker` operations, bulk
// symbol/type/reference queries, completions, diagnostics, cancellation, and failure behaviour.
//
// The checks below exercise the live candidate: API operations execute real RPCs against its native
// binary, while the cancellation check reflects over live client objects. Nothing is read from a type declaration.
import {
  resolveCandidate,
  loadSyncApi,
  makeFixture,
  offsetOf,
  check,
  checkThrows,
  record,
  section,
  assert,
  finish,
} from "./harness.mjs";

const candidate = resolveCandidate();
const api_mod = await loadSyncApi(candidate);
const { API } = api_mod;
const fx = makeFixture();

section(
  `probe5 bulk semantic API — typescript@${candidate.version} (gitHead ${candidate.gitHead})`,
);

const api = new API({ cwd: fx.root });
try {
  const snapshot = api.updateSnapshot({ openProjects: [fx.tsconfig] });

  // ---- project and source-file lookup -------------------------------------------------------
  section("5.1 project and source-file lookup");
  const project = snapshot.getProject(fx.tsconfig);
  check("getProject(tsconfig) resolves", () => {
    assert(project, "getProject returned undefined");
    return `configFileName=${project.configFileName}`;
  });
  check("getProjects() enumerates exactly the opened project", () => {
    const ps = snapshot.getProjects();
    assert(ps.length === 1, `expected 1 project, got ${ps.length}`);
    return `${ps.length} project(s)`;
  });
  check("getDefaultProjectForFile(main.ts) resolves to the configured project", () => {
    const p = snapshot.getDefaultProjectForFile(fx.main);
    assert(p, "no default project");
    assert(p.id === project.id, `default project ${p.id} != opened project ${project.id}`);
    return `id=${p.id}`;
  });
  check("getDefaultProjectForFile returns undefined for a file outside the project", () => {
    const p = snapshot.getDefaultProjectForFile("/definitely/not/here.ts");
    assert(p === undefined, `returned project ${p && p.id} for a file no project owns`);
    return "undefined";
  });

  const program = project.program;
  const checker = project.checker;

  check("program.getSourceFileNames()", () => {
    const names = program.getSourceFileNames();
    assert(names.length > 0, "empty");
    return `${names.length} file(s), includes main.ts: ${names.some((n) => n.endsWith("main.ts"))}`;
  });
  check("program.getSourceFile(main.ts) returns real text", () => {
    const sf = program.getSourceFile(fx.main);
    assert(sf, "undefined");
    assert(
      sf.text.length === fx.mainText.length,
      `text length ${sf.text.length} != ${fx.mainText.length}`,
    );
    return `${sf.text.length} chars`;
  });
  check("program.getSourceFile returns undefined for a nonexistent file", () => {
    const sf = program.getSourceFile("/definitely/not/here.ts");
    assert(sf === undefined, "returned a SourceFile for a path that does not exist");
    return "undefined (fail-soft, no throw)";
  });
  check("program.getSourceFileMetadata(main.ts) carries the documented field set", () => {
    const md = program.getSourceFileMetadata(fx.main);
    assert(md, "undefined");
    for (const k of ["isDefaultLibrary", "isFromExternalLibrary", "impliedNodeFormat"]) {
      assert(k in md, `missing ${k}; got [${Object.keys(md).join(",")}]`);
    }
    assert(md.isDefaultLibrary === false, "main.ts reported as a default-library file");
    return `keys=${Object.keys(md).join(",")}`;
  });
  check("program.getCompilerOptions() round-trips the fixture tsconfig", () => {
    const o = program.getCompilerOptions();
    assert(o.strict === true, `strict=${o.strict}, fixture sets true`);
    assert(o.declaration === true, `declaration=${o.declaration}, fixture sets true`);
    return `strict=${o.strict} declaration=${o.declaration}`;
  });
  check("program.getConfigFileNames() names the fixture tsconfig", () => {
    const names = program.getConfigFileNames();
    assert(names.length === 1 && names[0] === fx.tsconfig, `got [${names.join(",")}]`);
    return names[0];
  });
  check("isSourceFileDefaultLibrary discriminates", () => {
    const names = program.getSourceFileNames();
    let libs = 0,
      own = 0;
    for (const n of names) {
      const sf = program.getSourceFile(n);
      if (!sf) continue;
      if (program.isSourceFileDefaultLibrary(sf)) libs++;
      else own++;
    }
    assert(libs > 0 && own > 0, `not discriminating: libs=${libs} own=${own}`);
    return `${libs} default-lib, ${own} project file(s)`;
  });

  // ---- diagnostics --------------------------------------------------------------------------
  section("5.2 diagnostics (every documented kind, on a file with real errors)");
  // broken.ts seeds exactly two SEMANTIC errors (TS2322 + TS2355) and is syntactically valid, so every
  // other kind must be empty. Asserting the exact count per kind is what makes this discriminate: a
  // package that reclassified either error, or that started reporting it under another kind, goes red.
  const diagExpect = {
    getSyntacticDiagnostics: 0,
    getBindDiagnostics: 0,
    getSemanticDiagnostics: 2,
    getSuggestionDiagnostics: 0,
    getDeclarationDiagnostics: 0,
  };
  for (const [kind, expected] of Object.entries(diagExpect)) {
    check(`${kind}(broken.ts) returns exactly ${expected}`, () => {
      const ds = program[kind](fx.broken);
      const codes = ds.map((d) => d.code).join(",");
      assert(ds.length === expected, `expected ${expected}, got ${ds.length} [${codes}]`);
      return `${ds.length} diagnostic(s)${codes ? ` codes=[${codes}]` : ""}`;
    });
  }
  check("getSemanticDiagnostics(broken.ts) reports the seeded type error TS2322", () => {
    const ds = program.getSemanticDiagnostics(fx.broken);
    assert(
      ds.some((d) => d.code === 2322),
      `codes seen: ${ds.map((d) => d.code).join(",")}`,
    );
    const d = ds.find((d) => d.code === 2322);
    return `code=2322 category=${d.category} text=${JSON.stringify(String(d.messageText).slice(0, 60))}`;
  });
  check("getSemanticDiagnostics(main.ts) is clean", () => {
    const ds = program.getSemanticDiagnostics(fx.main);
    assert(ds.length === 0, `expected 0, got ${ds.length}: ${ds.map((d) => d.code).join(",")}`);
    return "0 diagnostics";
  });
  check(
    "getSemanticDiagnostics(bulk: [main, dep, broken]) equals the sum of the per-file calls",
    () => {
      const bulk = program.getSemanticDiagnostics([fx.main, fx.dep, fx.broken]);
      const perFile = [fx.main, fx.dep, fx.broken].reduce(
        (n, f) => n + program.getSemanticDiagnostics(f).length,
        0,
      );
      assert(bulk.length === perFile, `bulk ${bulk.length} != per-file total ${perFile}`);
      assert(bulk.length === 2, `expected 2, got ${bulk.length}`);
      return `${bulk.length} diagnostic(s) across 3 files in ONE call, matching the per-file total`;
    },
  );
  check("getSemanticDiagnostics() whole-program finds the seeded errors and no lib noise", () => {
    const ds = program.getSemanticDiagnostics();
    assert(
      ds.length === 2,
      `expected 2 program-wide, got ${ds.length} [${ds.map((d) => d.code).join(",")}]`,
    );
    return `${ds.length} diagnostic(s) program-wide`;
  });
  for (const kind of [
    "getProgramDiagnostics",
    "getGlobalDiagnostics",
    "getConfigFileParsingDiagnostics",
  ]) {
    check(`${kind} is empty for a well-formed project`, () => {
      const ds = program[kind]();
      assert(ds.length === 0, `expected 0, got ${ds.length} [${ds.map((d) => d.code).join(",")}]`);
      return "0 diagnostic(s)";
    });
  }
  check("diagnostic wire shape — the field set actually delivered", () => {
    const d = program.getSemanticDiagnostics(fx.broken)[0];
    assert(d, "no diagnostic");
    return `keys=[${Object.keys(d).sort().join(",")}]`;
  });
  check("diagnostic carries a resolvable position as pos/end (NOT start/length)", () => {
    const d = program.getSemanticDiagnostics(fx.broken).find((x) => x.code === 2322);
    assert(d, "TS2322 not found");
    assert(
      typeof d.pos === "number" && typeof d.end === "number",
      `pos=${d.pos} end=${d.end} (classic start/length: start=${d.start} length=${d.length})`,
    );
    assert(
      d.start === undefined && d.length === undefined,
      "classic start/length ARE present — the shape claim below is wrong",
    );
    const slice = fx.brokenText.slice(d.pos, d.end);
    return `pos=${d.pos} end=${d.end} covering ${JSON.stringify(slice)}; classic start/length absent`;
  });
  check("diagnostic message text rides `text`, not `messageText`", () => {
    const d = program.getSemanticDiagnostics(fx.broken).find((x) => x.code === 2322);
    assert(typeof d.text === "string" && d.text.length > 0, `text=${JSON.stringify(d.text)}`);
    assert(d.messageText === undefined, "messageText IS present");
    return `text=${JSON.stringify(d.text)} messageText=undefined`;
  });
  check("diagnostic carries fileName, so attribution needs no caller bookkeeping", () => {
    const d = program.getSemanticDiagnostics([fx.main, fx.dep, fx.broken])[0];
    assert(d.fileName, "fileName ABSENT — attribution WOULD need caller bookkeeping");
    assert(d.fileName.endsWith("broken.ts"), `attributed to ${d.fileName}, expected broken.ts`);
    return `fileName=${d.fileName.split("/").pop()}`;
  });

  // ---- Checker: single-value operations ------------------------------------------------------
  section("5.3 Checker single-value symbol/type operations");
  const makeWidgetPos = offsetOf(fx.mainText, "makeWidget");
  const widgetPos = offsetOf(fx.mainText, "Widget");

  check("checker.getSymbolAtPosition(main.ts, makeWidget decl)", () => {
    const sym = checker.getSymbolAtPosition(fx.main, makeWidgetPos);
    assert(sym, "undefined");
    return `name=${sym.name} flags=${sym.flags}`;
  });
  check("checker.getTypeAtPosition(main.ts, makeWidget) is a function type", () => {
    const t = checker.getTypeAtPosition(fx.main, makeWidgetPos);
    assert(t, "undefined");
    return `typeToString=${checker.typeToString(t)}`;
  });
  check("checker.getTypeOfSymbol round-trips through typeToString", () => {
    const sym = checker.getSymbolAtPosition(fx.main, makeWidgetPos);
    const t = checker.getTypeOfSymbol(sym);
    const s = checker.typeToString(t);
    assert(s.includes("Widget"), `expected the return type to mention Widget, got ${s}`);
    return s;
  });
  check("checker.getDeclaredTypeOfSymbol(Widget) enumerates its members", () => {
    const sym = checker.getSymbolAtPosition(fx.main, widgetPos);
    assert(sym, "no symbol at Widget");
    const t = checker.getDeclaredTypeOfSymbol(sym);
    const props = checker
      .getPropertiesOfType(t)
      .map((p) => p.name)
      .sort();
    assert(props.join(",") === "id,nested,size", `got ${props.join(",")}`);
    return `properties=[${props.join(",")}]`;
  });
  check("checker.getPropertyOfType(Widget, 'nested') then getTypeOfSymbol", () => {
    const sym = checker.getSymbolAtPosition(fx.main, widgetPos);
    const t = checker.getDeclaredTypeOfSymbol(sym);
    const nested = checker.getPropertyOfType(t, "nested");
    assert(nested, "no 'nested' property");
    return `nested: ${checker.typeToString(checker.getTypeOfSymbol(nested))}`;
  });
  check("checker.isTypeAssignableTo(string, string) === true", () => {
    const r = checker.isTypeAssignableTo(checker.getStringType(), checker.getStringType());
    assert(r === true, `expected true, got ${r}`);
    return "true";
  });
  check("checker.isTypeAssignableTo(string, number) === false (discriminates)", () => {
    const r = checker.isTypeAssignableTo(checker.getStringType(), checker.getNumberType());
    assert(r === false, `expected false, got ${r}`);
    return "false";
  });
  check("checker intrinsic type accessors each return their own type", () => {
    const expected = {
      getAnyType: "any",
      getStringType: "string",
      getNumberType: "number",
      getBooleanType: "boolean",
      getVoidType: "void",
      getUndefinedType: "undefined",
      getNullType: "null",
      getNeverType: "never",
      getUnknownType: "unknown",
      getBigIntType: "bigint",
    };
    const out = [];
    for (const [fn, want] of Object.entries(expected)) {
      const got = checker.typeToString(checker[fn]());
      assert(got === want, `${fn}() printed ${got}, expected ${want}`);
      out.push(`${want}`);
    }
    return out.join(" ");
  });
  check("checker.resolveName resolves a project symbol, and does NOT invent one", () => {
    const SymbolFlags_Value = 111551; // ts SymbolFlags.Value
    const loc = { document: fx.main, position: offsetOf(fx.mainText, "export const w") };
    const hit = checker.resolveName("makeWidget", SymbolFlags_Value, loc);
    assert(hit && hit.name === "makeWidget", `expected makeWidget, got ${hit && hit.name}`);
    const miss = checker.resolveName("noSuchSymbolAnywhere", SymbolFlags_Value, loc);
    assert(!miss, `resolved a name that does not exist: ${miss && miss.name}`);
    return `resolved ${hit.name}; unknown name resolved to undefined`;
  });
  check("checker.getSymbolOfSourceFile(dep.ts) then getExportsOfModule", () => {
    const modSym = checker.getSymbolOfSourceFile(fx.dep);
    assert(modSym, "no module symbol");
    const exports = checker
      .getExportsOfModule(modSym)
      .map((s) => s.name)
      .sort();
    assert(exports.includes("helper") && exports.includes("Shape"), `got ${exports.join(",")}`);
    return `exports=[${exports.join(",")}]`;
  });

  // ---- Checker: BULK (array-overload) operations ---------------------------------------------
  section("5.4 bulk symbol/type queries (array overloads — one round trip, many results)");
  const positions = [
    offsetOf(fx.mainText, "makeWidget"),
    offsetOf(fx.mainText, "Widget"),
    offsetOf(fx.mainText, "helper"),
    offsetOf(fx.mainText, "viaHelper"),
  ];
  check("checker.getSymbolAtPosition(file, positions[]) returns one entry per position", () => {
    const syms = checker.getSymbolAtPosition(fx.main, positions);
    assert(Array.isArray(syms), "not an array");
    assert(syms.length === positions.length, `${syms.length} != ${positions.length}`);
    return `[${syms.map((s) => (s ? s.name : "undefined")).join(", ")}]`;
  });
  check("checker.getTypeAtPosition(file, positions[]) returns one entry per position", () => {
    const types = checker.getTypeAtPosition(fx.main, positions);
    assert(types.length === positions.length, `${types.length} != ${positions.length}`);
    return `[${types.map((t) => (t ? checker.typeToString(t) : "undefined")).join(" | ")}]`;
  });
  check("checker.getTypeOfSymbol(symbols[]) bulk overload", () => {
    const syms = checker.getSymbolAtPosition(fx.main, positions).filter(Boolean);
    const types = checker.getTypeOfSymbol(syms);
    assert(types.length === syms.length, `${types.length} != ${syms.length}`);
    return `[${types.map((t) => checker.typeToString(t)).join(" | ")}]`;
  });
  check("checker.getSymbolOfSourceFile(files[]) bulk overload", () => {
    const syms = checker.getSymbolOfSourceFile([fx.main, fx.dep]);
    assert(syms.length === 2, `${syms.length} != 2`);
    return `[${syms.map((s) => (s ? s.name : "undefined")).join(", ")}]`;
  });
  check("bulk result order matches input order (positional contract)", () => {
    const shuffled = [positions[1], positions[0]];
    const syms = checker.getSymbolAtPosition(fx.main, shuffled);
    const single0 = checker.getSymbolAtPosition(fx.main, shuffled[0]);
    const single1 = checker.getSymbolAtPosition(fx.main, shuffled[1]);
    assert(
      syms[0]?.name === single0?.name && syms[1]?.name === single1?.name,
      `bulk [${syms.map((s) => s?.name)}] vs single [${single0?.name}, ${single1?.name}]`,
    );
    return `order preserved: [${syms.map((s) => s?.name).join(", ")}]`;
  });
  check("empty bulk input returns an empty array, not an error", () => {
    const syms = checker.getSymbolAtPosition(fx.main, []);
    assert(Array.isArray(syms) && syms.length === 0, `got ${JSON.stringify(syms)}`);
    return "[]";
  });

  // ---- references ---------------------------------------------------------------------------
  section("5.5 reference queries");
  check("same-file references: getReferencesToSymbolInFile(main.ts, makeWidget)", () => {
    const sym = checker.getSymbolAtPosition(fx.main, makeWidgetPos);
    const refs = checker.getReferencesToSymbolInFile(fx.main, sym);
    assert(refs.length >= 3, `expected >=3 (decl + 3 call sites), got ${refs.length}`);
    return `${refs.length} reference handle(s)`;
  });
  check(
    "there is NO project-wide references primitive: the declaration symbol finds nothing in an importing file",
    () => {
      const depHelperPos =
        offsetOf(fx.depText, "export function helper") + "export function ".length;
      const declSym = checker.getSymbolAtPosition(fx.dep, depHelperPos);
      assert(declSym && declSym.name === "helper", `wrong symbol: ${declSym && declSym.name}`);
      const inDep = checker.getReferencesToSymbolInFile(fx.dep, declSym);
      const inMain = checker.getReferencesToSymbolInFile(fx.main, declSym);
      assert(
        inDep.length > 0,
        `expected the declaration to be found in its own file, got ${inDep.length}`,
      );
      assert(
        inMain.length === 0,
        `dep.ts#helper WAS found in main.ts (${inMain.length}) — the per-file-symbol-identity finding does not reproduce`,
      );
      return `dep.ts: ${inDep.length} ref(s); main.ts: ${inMain.length} ref(s) — main.ts uses helper, yet the declaration symbol matches nothing there`;
    },
  );
  check("cross-file references must be assembled per file via that file's own ALIAS symbol", () => {
    const impPos = offsetOf(fx.mainText, "import { helper") + "import { ".length;
    const aliasSym = checker.getSymbolAtPosition(fx.main, impPos);
    assert(aliasSym, "no symbol at the import site");
    const SymbolFlags_Alias = 2097152;
    assert(
      (aliasSym.flags & SymbolFlags_Alias) !== 0,
      `import-site symbol is not an Alias (flags=${aliasSym.flags})`,
    );
    const refs = checker.getReferencesToSymbolInFile(fx.main, aliasSym);
    assert(refs.length >= 2, `expected >=2 (import specifier + use site), got ${refs.length}`);
    const aliased = checker.getAliasedSymbol(aliasSym);
    const viaAliased = checker.getReferencesToSymbolInFile(fx.main, aliased);
    assert(
      viaAliased.length === 0,
      `the RESOLVED alias target found ${viaAliased.length} ref(s) in main.ts — it should find none`,
    );
    return `alias symbol: ${refs.length} ref(s) in main.ts; its resolved target: ${viaAliased.length}. A project-wide "find all references" is caller-assembled, not a server primitive.`;
  });
  check(
    "getReferencedSymbolsForNode fails SOFT (empty) when handed a SourceFile instead of an identifier node",
    () => {
      const sf = program.getSourceFile(fx.main);
      const refs = project.languageService.getReferencedSymbolsForNode(sf, makeWidgetPos);
      assert(
        refs.length === 0,
        `returned ${refs.length} — the fail-soft finding does not reproduce`,
      );
      return "0 entries, no error — an empty result here is indistinguishable from 'no references'";
    },
  );

  // ---- completions --------------------------------------------------------------------------
  section("5.6 completions");
  check(
    "member-access completion (no auto-imports needed) SUCCEEDS and lists the member set",
    () => {
      const pos = offsetOf(fx.mainText, "export const idOf = w.") + "export const idOf = w.".length;
      const info = project.languageService.getCompletionsAtPosition(fx.main, pos);
      assert(info, "undefined completion info");
      const names = (info.entries ?? []).map((e) => e.name).sort();
      assert(
        names.includes("id") && names.includes("size") && names.includes("nested"),
        `expected Widget members, got [${names.join(",")}]`,
      );
      return `${names.length} entr(ies): [${names.join(",")}]`;
    },
  );
  check("identifier-position completion REJECTS with 'completion list needs auto imports'", () => {
    // GUARDS evidence 6.2(c), declared binding on TCM2/TCM3. If this package ever serves
    // identifier-position completions instead of refusing them, 6.2(c) is wrong and this MUST go red.
    const pos = offsetOf(fx.mainText, "export const repeated");
    let threw = false,
      message = "";
    try {
      const info = project.languageService.getCompletionsAtPosition(fx.main, pos);
      throw new Error(
        `returned ${info?.entries?.length ?? 0} entries instead of refusing — evidence 6.2(c) ` +
          `("rejects any completion list needing auto-imports") no longer holds`,
      );
    } catch (err) {
      message = err.message;
      threw = /auto import/i.test(message);
    }
    assert(threw, message);
    return `threw: ${message}`;
  });
  check("includeSymbol attaches a Symbol handle; omitting it does not", () => {
    const pos = offsetOf(fx.mainText, "export const idOf = w.") + "export const idOf = w.".length;
    const withOpt = project.languageService.getCompletionsAtPosition(fx.main, pos, {
      includeSymbol: true,
    });
    const withCount = (withOpt.entries ?? []).filter((e) => e.symbol).length;
    assert(
      withCount === (withOpt.entries ?? []).length && withCount > 0,
      `${withCount}/${withOpt.entries?.length ?? 0} entries carried a Symbol`,
    );
    const without = project.languageService.getCompletionsAtPosition(fx.main, pos);
    const withoutCount = (without.entries ?? []).filter((e) => e.symbol).length;
    assert(withoutCount === 0, `${withoutCount} entries carried a Symbol without includeSymbol`);
    return `${withCount}/${withOpt.entries.length} with the option, ${withoutCount} without`;
  });

  // ---- emit / declaration -------------------------------------------------------------------
  section("5.7 emit and declaration output");
  check("program.getDeclarationEmit([main.ts]) yields a .d.ts with real content", () => {
    const out = program.getDeclarationEmit([fx.main]);
    const entries = [...out.outputFiles.entries()];
    assert(entries.length > 0, `emitSkipped=${out.emitSkipped}, 0 output files`);
    const dts = entries.find(([name]) => name.endsWith(".d.ts"));
    assert(dts, `no .d.ts emitted; got ${entries.map(([n]) => n).join(",")}`);
    const [dtsName, dtsFile] = dts;
    assert(dtsFile.text.includes("Widget"), "the emitted .d.ts does not mention Widget");
    assert(
      dtsFile.fileName === undefined,
      "EmitOutputFile carries a fileName field — the map-key-is-the-name finding is wrong",
    );
    return `${entries.length} file(s); ${dtsName.split("/").pop()} = ${dtsFile.text.length} chars; value keys=[${Object.keys(dtsFile).join(",")}] (name is the MAP KEY)`;
  });
  check("program.getJavaScriptEmit([main.ts]) yields JS", () => {
    const out = program.getJavaScriptEmit([fx.main]);
    const entries = [...out.outputFiles.entries()];
    assert(entries.length > 0, `emitSkipped=${out.emitSkipped}, 0 output files`);
    return `${entries.length} file(s): ${entries.map(([n]) => n.split("/").pop()).join(",")}`;
  });

  // ---- cancellation -------------------------------------------------------------------------
  section("5.8 cancellation (absence, probed on the live objects rather than the .d.ts)");
  check("no cancellation member on API / Snapshot / Program / Checker / LanguageService", () => {
    const targets = {
      API: api,
      Snapshot: snapshot,
      Program: program,
      Checker: checker,
      LanguageService: project.languageService,
    };
    const hits = [];
    for (const [label, obj] of Object.entries(targets)) {
      let proto = obj;
      const seen = new Set();
      while (proto && proto !== Object.prototype) {
        for (const k of Object.getOwnPropertyNames(proto)) {
          if (seen.has(k)) continue;
          seen.add(k);
          if (/cancel|abort/i.test(k)) hits.push(`${label}.${k}`);
        }
        proto = Object.getPrototypeOf(proto);
      }
    }
    assert(hits.length === 0, `found cancellation-shaped members: ${hits.join(", ")}`);
    return "none — no cancel/abort member anywhere on the live session objects";
  });

  // ---- failure behaviour --------------------------------------------------------------------
  section("5.9 failure behaviour");
  check("a whitespace position degrades to the FILE's module symbol, not undefined", () => {
    const sym = checker.getSymbolAtPosition(fx.main, offsetOf(fx.mainText, "\n\n"));
    assert(sym, "returned undefined — the degrade-to-module-symbol finding does not reproduce");
    const bare = sym.name.replace(/^"|"$/g, "");
    assert(bare.endsWith("/main"), `expected the module symbol for main.ts, got ${sym.name}`);
    return `returned the module symbol (name is the QUOTED module path, ${JSON.stringify(sym.name.slice(0, 1))}…) — a caller cannot distinguish "no symbol here" from "the file itself"`;
  });
  check("a beyond-EOF position ALSO degrades to the module symbol rather than failing", () => {
    // GUARDS evidence 6.2(e), declared binding on TCM2/TCM3 (positions must be clamped Verter-side
    // because the callee does not validate). If this path ever starts failing closed, 6.2(e)'s
    // justification is gone and this MUST go red rather than quietly reporting the better behaviour.
    let sym, threwMessage;
    try {
      sym = checker.getSymbolAtPosition(fx.main, fx.mainText.length + 100000);
    } catch (err) {
      threwMessage = err.message;
    }
    assert(
      threwMessage === undefined,
      `it FAILED CLOSED (${threwMessage}) — the callee now validates range, so evidence 6.2(e) ` +
        `("NO range validation on this path") no longer holds`,
    );
    assert(sym, "returned undefined rather than degrading to a symbol — 6.2(e) no longer holds");
    const bare = sym.name.replace(/^"|"$/g, "");
    assert(bare.endsWith("/main"), `expected the module symbol, got ${sym.name}`);
    return "returned the module symbol for a position 100000 chars past EOF — NO range validation on this path";
  });
  checkThrows("getSemanticDiagnostics on a file not in the project fails closed", () =>
    program.getSemanticDiagnostics("/definitely/not/here.ts"),
  );
  checkThrows("api.parseConfigFile on a nonexistent tsconfig fails closed", () =>
    api.parseConfigFile("/definitely/not/here/tsconfig.json"),
  );

  section("5.10 disposal fail-closed");
  const s2 = api.updateSnapshot({ openProjects: [fx.tsconfig] });
  const p2 = s2.getProject(fx.tsconfig);
  const checker2 = p2.checker;
  s2.dispose();
  checkThrows("Snapshot.getProject after dispose", () => s2.getProject(fx.tsconfig));
  checkThrows("Checker.getSymbolAtPosition after its snapshot is disposed fails closed", () =>
    checker2.getSymbolAtPosition(fx.main, makeWidgetPos),
  );

  snapshot.dispose();
} finally {
  try {
    api.close();
  } catch {}
  fx.dispose();
}
finish();
