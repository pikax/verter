// Self-test: the TypeScript observation DOMAIN — the realized, pinned
// framework closure that gives an artifact's `import("vue")` /
// `import("svelte")` references their meaning.
//
// Without a domain, TypeScript silently types an unresolvable framework
// reference `any` inside a `.d.ts` under `skipLibCheck`, and two declarations
// that differ ONLY in their framework types observe IDENTICALLY. An
// observation taken in that state decides nothing. These tests prove the five
// conditions that make a domain-backed observation decisive:
//
// 1. the exact pinned declaration closure is resolvable by the existing host;
// 2. two observations run under an identical TypeScript version, options,
//    framework closure and module-resolution environment — and the record
//    carries that identity, so a drift in any of it is reported;
// 3. module-resolution failure REFUSES the observation instead of degrading;
// 4. a planted control proves a correct prop surface and an empty one produce
//    DIFFERENT observations;
// 5. the observation is semantic — props, exports and bindings are read from
//    the checker, not from the declaration's bytes.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import ts from "typescript";

import { describe, expect, it } from "vitest";

import {
  ModuleResolutionError,
  compareObservations,
  observeTypeScript,
  toIdentityPath,
} from "../src/typescript-observe.mjs";
import { SVELTE_DOMAIN, VUE_DOMAIN } from "../src/domain-pin.mjs";

/** A Svelte declaration in the shape the Svelte API projector renders. */
function svelteShim({ props, exports, bindings }) {
  return (
    `declare const Api: import("svelte").Component<\n  ${props},\n  ${exports},\n  ${bindings}\n>;\n` +
    "export default Api;\n"
  );
}

const TYPED_PROPS = svelteShim({
  props: "{ label: string; disabled?: boolean }",
  exports: "{ focus: () => void }",
  bindings: '""',
});
const EMPTY_PROPS = svelteShim({ props: "{}", exports: "{}", bindings: '""' });

/** A Vue declaration in the shape the Vue API projector renders. */
const VUE_DECLARATION =
  'type __Verter_RootElementAttrs<Tag extends string> = Tag extends keyof import("vue").IntrinsicElementAttributes ? import("vue").IntrinsicElementAttributes[Tag] : {}\n' +
  'declare module "vue" {\n  interface IntrinsicElementAttributes {}\n}\n' +
  "declare const App: {\n" +
  '  new(props?: import("vue").PublicProps & { label: string; disabled?: boolean }): {\n' +
  '    $props: import("vue").PublicProps & { label: string; disabled?: boolean },\n' +
  '    $emit: ((event: "toggle", ...args: unknown[]) => void),\n' +
  "  }\n}\nexport default App\n";

describe("TypeScript observation domain — the pinned framework closure", () => {
  it("(1) resolves the exact pinned Svelte and Vue declaration closures", () => {
    const svelte = observeTypeScript([{ fileName: "/Api.d.ts", code: TYPED_PROPS }], {
      frameworkDomain: "svelte",
    });
    const vue = observeTypeScript([{ fileName: "/App.d.ts", code: VUE_DECLARATION }], {
      frameworkDomain: "vue",
    });
    // Resolution succeeded (an unresolved reference would have thrown), and the
    // closure that supplied the meaning is the PINNED one.
    expect(svelte.observationDomain).toEqual({
      framework: "svelte",
      packageVersion: SVELTE_DOMAIN.packageVersion,
    });
    expect(vue.observationDomain).toEqual({
      framework: "vue",
      packageVersion: VUE_DOMAIN.packageVersion,
    });
    expect(svelte.diagnostics).toEqual([]);
    expect(vue.diagnostics).toEqual([]);
  });

  it("(2) two observations of identical artifacts share a queryIdentity, and a domain change breaks it", () => {
    const a = observeTypeScript([{ fileName: "/Api.d.ts", code: TYPED_PROPS }], {
      frameworkDomain: "svelte",
    });
    const b = observeTypeScript([{ fileName: "/Api.d.ts", code: TYPED_PROPS }], {
      frameworkDomain: "svelte",
    });
    expect(a.queryIdentity).toBe(b.queryIdentity);
    expect(compareObservations(a, b).equal).toBe(true);

    // The domain is part of the identity: the same artifact observed in a
    // DIFFERENT domain is not a comparable result of the same query. (This
    // artifact references no module, so both domains resolve it.)
    const domainless = observeTypeScript([
      { fileName: "/plain.ts", code: "export const n = 1;\n" },
    ]);
    const inSvelte = observeTypeScript([{ fileName: "/plain.ts", code: "export const n = 1;\n" }], {
      frameworkDomain: "svelte",
    });
    expect(domainless.queryIdentity).not.toBe(inSvelte.queryIdentity);
    const drift = compareObservations(domainless, inSvelte);
    expect(drift.equal).toBe(false);
    expect(drift.differences.some((d) => d.includes("observationDomain"))).toBe(true);
  });

  it("(3) an unresolvable module reference REFUSES the observation instead of degrading to any", () => {
    // The exact silent-degradation failure: this artifact observed WITHOUT a domain used
    // to return a record in which `Api` was `any`.
    expect(() => observeTypeScript([{ fileName: "/Api.d.ts", code: TYPED_PROPS }])).toThrow(
      ModuleResolutionError,
    );
    try {
      observeTypeScript([{ fileName: "/Api.d.ts", code: TYPED_PROPS }]);
      expect.unreachable("the observation must be refused");
    } catch (error) {
      expect(error).toBeInstanceOf(ModuleResolutionError);
      expect(error.unresolved).toEqual([{ fileName: "/Api.d.ts", specifier: "svelte" }]);
    }

    // A `declare module "x"` augmentation of a module that does not exist is
    // the same silent-degradation case, and is refused too.
    expect(() =>
      observeTypeScript(
        [
          {
            fileName: "/aug.d.ts",
            code: 'declare module "not-a-real-package" { interface X {} }\nexport {};\n',
          },
        ],
        { frameworkDomain: "svelte" },
      ),
    ).toThrow(ModuleResolutionError);
  });

  // Every reference FORM TypeScript can resolve, each with its own planted
  // control. The enumeration is TypeScript's own — `ts.preProcessFile` is what
  // the gate asks — so this table checks each channel is actually wired rather
  // than guessing at completeness. A `require()` call, an `import =`, a dynamic
  // `import()`, and a `/// <reference types>` all slipped through the earlier
  // hand-written AST walk and yielded `any`.
  const UNRESOLVABLE_FORMS = [
    ["import declaration", 'import x from "verter-no-such-package-a";\nexport const a = x;\n'],
    ["export … from", 'export { y } from "verter-no-such-package-b";\n'],
    ["export *", 'export * from "verter-no-such-package-c";\n'],
    ["import() type node", 'export type T = import("verter-no-such-package-d").Thing;\n'],
    [
      "module augmentation",
      'declare module "verter-no-such-package-e" { interface I {} }\nexport {};\n',
    ],
    [
      "require() call",
      'declare const require: any;\nexport const r = require("verter-no-such-package-f");\n',
    ],
    ["import = require()", 'import a = require("verter-no-such-package-g");\nexport { a };\n'],
    ["dynamic import()", 'export const p = import("verter-no-such-package-h");\n'],
    [
      "/// <reference types>",
      '/// <reference types="verter-no-such-package-i" />\nexport const z = 1;\n',
    ],
    ["typeof import()", 'export declare const v: typeof import("verter-no-such-package-j");\n'],
    [
      "/// <reference path>",
      '/// <reference path="./verter-no-such-package-k.d.ts" />\nexport const w = 1;\n',
    ],
    [
      "/// <reference lib>",
      '/// <reference lib="verter-no-such-package-l" />\nexport const u = 1;\n',
    ],
  ];

  it.each(UNRESOLVABLE_FORMS)("(3b) an unresolvable %s REFUSES the observation", (_form, code) => {
    // The plant is the unresolvable specifier itself: present in this artifact
    // and, by construction, in no package on disk.
    const planted = code.match(/verter-no-such-package-[a-l]/)[0];
    let refusal;
    try {
      observeTypeScript([{ fileName: "/form-probe.ts", code }]);
    } catch (error) {
      refusal = error;
    }
    expect(refusal, "the form was not refused, so it yields `any` silently").toBeInstanceOf(
      ModuleResolutionError,
    );
    // The recorded specifier is the reference AS WRITTEN — a bare package name
    // for the module forms, a relative path for `/// <reference path>` — so the
    // planted token is matched inside it rather than against it.
    expect(
      refusal.unresolved.some((entry) => entry.specifier.includes(planted)),
      `no refusal names the planted specifier; got ${JSON.stringify(
        refusal.unresolved.map((entry) => entry.specifier),
      )}`,
    ).toBe(true);
  });

  it("(3c) the same forms PASS once their specifier resolves, so the gate is not refusing everything", () => {
    const resolvable = [
      'import { mount } from "svelte";\nexport const a = mount;\n',
      'export type T = import("svelte").Component;\n',
      'declare module "svelte" { interface VerterProbe {} }\nexport {};\n',
      'export declare const v: typeof import("svelte");\n',
    ];
    for (const code of resolvable) {
      expect(() =>
        observeTypeScript([{ fileName: "/resolvable.ts", code }], { frameworkDomain: "svelte" }),
      ).not.toThrow();
    }

    // The two directive channels need their own complements: a `path` reference
    // to a file that IS in the observed set, and a `lib` name the compiler
    // itself knows. Without these the new gates could be refusing everything.
    expect(() =>
      observeTypeScript([
        { fileName: "/sibling.d.ts", code: "export declare const s: number;\n" },
        {
          fileName: "/resolvable-path.ts",
          code: '/// <reference path="./sibling.d.ts" />\nexport const b = 1;\n',
        },
      ]),
    ).not.toThrow();
    expect(() =>
      observeTypeScript([
        {
          fileName: "/resolvable-lib.ts",
          code: '/// <reference lib="es2015" />\nexport const c = 1;\n',
        },
      ]),
    ).not.toThrow();
  });

  // @ai-generated - Proves path references cannot read identity-free declarations from disk.
  it("(3d) an on-disk path reference REFUSES until its target enters the observation map", () => {
    const fixtureDir = mkdtempSync(path.join(tmpdir(), "verter-ts-observation-"));
    const apiPath = path.join(fixtureDir, "Api.d.ts");
    const targetPath = path.join(fixtureDir, "external.d.ts");
    const apiCode =
      '/// <reference path="./external.d.ts" />\n' + "export declare const observed: External;\n";
    const targetOne = 'interface External { value: "one"; }\n';
    const targetTwo = 'interface External { value: "two"; }\n';

    try {
      writeFileSync(targetPath, targetOne, "utf8");

      let refusal;
      try {
        observeTypeScript([{ fileName: apiPath, code: apiCode }]);
      } catch (error) {
        refusal = error;
      }
      expect(refusal, "the untracked declaration was read from disk").toBeInstanceOf(
        ModuleResolutionError,
      );
      expect(refusal.unresolved).toEqual([{ fileName: apiPath, specifier: "./external.d.ts" }]);

      const mappedOne = observeTypeScript([
        { fileName: apiPath, code: apiCode },
        { fileName: targetPath, code: targetOne },
      ]);
      const mappedTwo = observeTypeScript([
        { fileName: apiPath, code: apiCode },
        { fileName: targetPath, code: targetTwo },
      ]);

      expect(mappedOne.modules[apiPath].exports.observed.type.members.value.display).toBe('"one"');
      expect(mappedTwo.modules[apiPath].exports.observed.type.members.value.display).toBe('"two"');
      expect(mappedTwo.queryIdentity).not.toBe(mappedOne.queryIdentity);
    } finally {
      rmSync(fixtureDir, { recursive: true, force: true });
    }
  });

  // @ai-generated - Proves Windows path resolution cannot diverge from portable map identity.
  it("(3e) a Windows path reference admits only its portable mapped identity", () => {
    const apiPath = "C:\\Api.d.ts";
    const targetPath = "C:\\external.d.ts";
    const apiCode =
      '/// <reference path="./external.d.ts" />\n' + "export declare const observed: External;\n";
    const targetCode = 'interface External { value: "mapped"; }\n';

    expect(path.win32.resolve(path.win32.dirname(apiPath), "./external.d.ts")).toBe(targetPath);
    const mapped = observeTypeScript([
      { fileName: apiPath, code: apiCode },
      { fileName: targetPath, code: targetCode },
    ]);
    expect(mapped.modules[apiPath].exports.observed.type.members.value.display).toBe('"mapped"');

    let refusal;
    try {
      observeTypeScript([{ fileName: apiPath, code: apiCode }]);
    } catch (error) {
      refusal = error;
    }
    expect(refusal, "an untracked Windows-shaped target was admitted").toBeInstanceOf(
      ModuleResolutionError,
    );
    expect(refusal.unresolved).toEqual([{ fileName: apiPath, specifier: "./external.d.ts" }]);
  });

  it("(3f) a WINDOWS-SHAPED path normalizes to a backslash-free identity", () => {
    // The normalization exists for Windows, where `path.relative` yields
    // backslashes. On a POSIX runner `path.sep` is already `/`, so a test that
    // only drove the platform separator would exercise NOTHING. This drives the
    // Windows shape directly, so the rule is checked on every platform.
    const windowsShaped = "pkg\\sub\\Comp.svelte.d.ts";
    expect(windowsShaped).toContain("\\");

    const identity = toIdentityPath(windowsShaped);

    expect(identity, "the identity still carries a platform separator").not.toContain("\\");
    expect(identity).toBe("/pkg/sub/Comp.svelte.d.ts");
    // The POSIX shape of the same path must produce the SAME identity, which is
    // the whole point: one observation, one identity, every platform.
    expect(toIdentityPath("pkg/sub/Comp.svelte.d.ts")).toBe(identity);
    // Repeated and mixed separators collapse the same way.
    expect(toIdentityPath("pkg\\\\sub//Comp.svelte.d.ts")).toBe(identity);
  });

  // @ai-generated - Proves the real compiler host loads Windows-spelled virtual roots and imports.
  it("(3g) Windows-spelled virtual files load and retain their caller report identity", () => {
    const apiPath = "C:\\verter-virtual\\Api.ts";
    const dependencyPath = "C:\\verter-virtual\\Dependency.ts";
    const observation = observeTypeScript([
      {
        fileName: apiPath,
        code: 'export { observed } from "./Dependency";\n',
      },
      {
        fileName: dependencyPath,
        code: 'export const observed = "windows-root-loaded" as const;\n',
      },
    ]);

    expect(observation.diagnostics.some((diagnostic) => diagnostic.code === 6053)).toBe(false);
    expect(observation.diagnostics).toEqual([]);
    expect(observation.inputs.map((input) => input.fileName)).toEqual([apiPath, dependencyPath]);
    expect(Object.keys(observation.modules)).toEqual([apiPath, dependencyPath]);
    expect(observation.modules[apiPath].exports.observed.type.display).toBe(
      '"windows-root-loaded"',
    );
    expect(observation.modules[apiPath.replaceAll("\\", "/")]).toBeUndefined();
  });

  // @ai-generated - Proves separator aliases cannot silently overwrite a virtual artifact.
  it("(3h) separator aliases REFUSE an ambiguous canonical virtual identity", () => {
    const fileNames = ["C:\\alias\\Api.ts", "C:/alias/Api.ts"];
    let observation;
    let refusal;
    try {
      observation = observeTypeScript([
        { fileName: fileNames[0], code: 'export const selected = "backslash" as const;\n' },
        { fileName: fileNames[1], code: 'export const selected = "slash" as const;\n' },
      ]);
    } catch (error) {
      refusal = error;
    }

    expect(observation, "one alias silently replaced the other").toBeUndefined();
    expect(refusal).toMatchObject({ name: "VirtualFileIdentityError" });
    expect(refusal.collisions).toEqual([{ fileNames: [...fileNames].sort() }]);
    expect(refusal.message).toContain(JSON.stringify(fileNames[0]));
    expect(refusal.message).toContain(JSON.stringify(fileNames[1]));
  });

  // @ai-generated - Proves case aliases follow the compiler host's filesystem semantics.
  it("(3i) case aliases are refused only when the host identity is case-insensitive", async () => {
    const fileNames = ["/virtual/Case.ts", "/virtual/case.ts"];
    const artifacts = [
      { fileName: fileNames[0], code: 'export const upper = "upper" as const;\n' },
      { fileName: fileNames[1], code: 'export const lower = "lower" as const;\n' },
    ];

    const hostCaseSensitivity = ts.sys.useCaseSensitiveFileNames;
    ts.sys.useCaseSensitiveFileNames = true;
    let caseSensitiveObservation;
    try {
      const { observeTypeScript: caseSensitiveObserveTypeScript } =
        await import("../src/typescript-observe.mjs?case-sensitive-host");
      caseSensitiveObservation = caseSensitiveObserveTypeScript(artifacts);
    } finally {
      ts.sys.useCaseSensitiveFileNames = hostCaseSensitivity;
    }
    expect(caseSensitiveObservation.inputs.map(({ fileName }) => fileName)).toEqual(
      [...fileNames].sort((left, right) => left.localeCompare(right)),
    );
    const observedModuleNames = Object.keys(caseSensitiveObservation.modules);
    expect(observedModuleNames).toHaveLength(fileNames.length);
    expect(new Set(observedModuleNames)).toEqual(new Set(fileNames));
    expect(caseSensitiveObservation.modules[fileNames[0]].exports.upper.type.display).toBe(
      '"upper"',
    );
    expect(caseSensitiveObservation.modules[fileNames[1]].exports.lower.type.display).toBe(
      '"lower"',
    );
    expect(caseSensitiveObservation.modules[fileNames[0]].exports.lower).toBeUndefined();
    expect(caseSensitiveObservation.modules[fileNames[1]].exports.upper).toBeUndefined();

    if (hostCaseSensitivity) {
      return;
    }

    let observation;
    let refusal;
    try {
      observation = observeTypeScript(artifacts);
    } catch (error) {
      refusal = error;
    }
    expect(observation, "one case alias silently replaced the other").toBeUndefined();
    expect(refusal).toMatchObject({ name: "VirtualFileIdentityError" });
    expect(refusal.collisions).toEqual([{ fileNames: [...fileNames].sort() }]);
  });

  // @ai-generated - Proves absolute path references stay absolute while relative references stay local.
  it("(3j) mapped root-, drive-, and relative path references resolve by canonical identity", () => {
    const rootApi = "/rooted/Api.ts";
    const driveApi = "C:\\drive\\Api.ts";
    const observation = observeTypeScript([
      {
        fileName: rootApi,
        code:
          '/// <reference path="/shared/root.d.ts" />\n' +
          '/// <reference path="./relative.d.ts" />\n' +
          "export declare const rooted: RootAbsolute;\n" +
          "export declare const relative: RelativeTarget;\n",
      },
      {
        fileName: driveApi,
        code:
          '/// <reference path="C:\\shared\\drive.d.ts" />\n' +
          "export declare const driven: DriveAbsolute;\n",
      },
      {
        fileName: "/shared/root.d.ts",
        code: 'interface RootAbsolute { value: "root"; }\n',
      },
      {
        fileName: "/rooted/relative.d.ts",
        code: 'interface RelativeTarget { value: "relative"; }\n',
      },
      {
        fileName: "C:\\shared\\drive.d.ts",
        code: 'interface DriveAbsolute { value: "drive"; }\n',
      },
    ]);

    expect(observation.diagnostics).toEqual([]);
    expect(observation.modules[rootApi].exports.rooted.type.members.value.display).toBe('"root"');
    expect(observation.modules[rootApi].exports.relative.type.members.value.display).toBe(
      '"relative"',
    );
    expect(observation.modules[driveApi].exports.driven.type.members.value.display).toBe('"drive"');
    expect(observation.modules["/rooted/shared/root.d.ts"]).toBeUndefined();
    expect(observation.modules["C:\\drive\\C:\\shared\\drive.d.ts"]).toBeUndefined();
  });

  // @ai-generated - Proves every diagnostic source retains caller identity after relocation.
  it("(3k) related diagnostic locations retain Windows-shaped caller identities", () => {
    const apiPath = "C:\\caller\\Api.ts";
    const dependencyPath = "C:\\caller\\Dependency.ts";
    const observation = observeTypeScript(
      [
        {
          fileName: apiPath,
          code:
            'import type { Config } from "./Dependency";\n' +
            "export const config: Config = { value: 123 };\n",
        },
        {
          fileName: dependencyPath,
          code: "export interface Config { value: string; }\n",
        },
      ],
      { frameworkDomain: "svelte" },
    );

    expect(observation.diagnostics).toHaveLength(1);
    const diagnostic = observation.diagnostics[0];
    expect(diagnostic.code).toBe(2322);
    expect(diagnostic.source).toBe(apiPath);
    expect(diagnostic.related).toHaveLength(1);
    expect(diagnostic.related[0].source).toBe(dependencyPath);
    const serialized = JSON.stringify(diagnostic);
    expect(serialized).not.toContain(apiPath.replaceAll("\\", "/"));
    expect(serialized).not.toContain(dependencyPath.replaceAll("\\", "/"));
    expect(serialized).not.toContain(".oracle-installs");
    expect(serialized).not.toContain("__verter_observed__");
  });

  it("(4) PLANTED CONTROL: a correct prop surface and an empty one observe DIFFERENTLY", () => {
    // Plant proven applied: the two declarations differ, and the planted
    // marker (`label`) is present in exactly one of them.
    expect(EMPTY_PROPS).not.toBe(TYPED_PROPS);
    expect(TYPED_PROPS.match(/label/g)).toHaveLength(1);
    expect(EMPTY_PROPS.match(/label/g)).toBeNull();

    const correct = observeTypeScript([{ fileName: "/Api.d.ts", code: TYPED_PROPS }], {
      frameworkDomain: "svelte",
    });
    const empty = observeTypeScript([{ fileName: "/Api.d.ts", code: EMPTY_PROPS }], {
      frameworkDomain: "svelte",
    });
    const comparison = compareObservations(correct, empty);
    expect(comparison.equal).toBe(false);
    // Attributed to the props parameter, not merely "something differs".
    expect(comparison.differences.some((difference) => difference.includes("label"))).toBe(true);
    // Both sides are diagnostic-clean, so ONLY the observed type differs —
    // exactly the drift class a byte comparison would miss.
    expect(correct.diagnostics).toEqual([]);
    expect(empty.diagnostics).toEqual([]);
  });

  it("(5) the observation is SEMANTIC: props, exports and bindings come from the checker", () => {
    const observation = observeTypeScript([{ fileName: "/Api.d.ts", code: TYPED_PROPS }], {
      frameworkDomain: "svelte",
    });
    const component = observation.modules["/Api.d.ts"].exports.default.type;
    const call = component.callSignatures?.[0];
    expect(call, "the pinned Component contract is callable").toBeDefined();

    // Props: the SECOND parameter of the native `Component` call signature,
    // structurally expanded by the checker.
    const props = call.parameters[1];
    expect(Object.keys(props.members ?? {}).sort()).toEqual(["disabled", "label"]);
    expect(props.members.label.display).toBe("string");
    expect(props.members.disabled.optional).toBe(true);

    // Exports: the call signature's RETURN type. The pinned `Component`
    // contract adds its own legacy-API members (`$on` / `$set`) alongside the
    // declared export — their presence is itself proof the PINNED contract was
    // consulted rather than the declaration's bytes read back.
    expect(Object.keys(call.returnType.members ?? {}).sort()).toEqual(["$on", "$set", "focus"]);
    expect(call.returnType.members.focus.display).toBe("() => void");

    // Bindings: the pinned contract surfaces the third generic argument as a
    // member of the component value.
    expect(component.members?.z_$$bindings?.display).toContain('""');
  });

  it("(5b) a Vue declaration's props and emits are read semantically through the pinned vue types", () => {
    const observation = observeTypeScript([{ fileName: "/App.d.ts", code: VUE_DECLARATION }], {
      frameworkDomain: "vue",
    });
    const app = observation.modules["/App.d.ts"].exports.default.type;
    const instance = app.constructSignatures?.[0]?.returnType;
    expect(instance, "the declaration is constructible").toBeDefined();
    // `$props` is the intersection of the pinned `PublicProps` with the
    // declared surface — proof the pinned types were actually consulted.
    expect(instance.members.$props.display).toContain("label: string");
    expect(instance.members.$props.display).toContain("VNodeProps");
    // `$emit` keeps its literal event name.
    expect(instance.members.$emit.display).toContain('"toggle"');
  });
});
