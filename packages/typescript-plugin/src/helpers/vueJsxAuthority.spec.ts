import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import ts from "typescript";
import { afterEach, describe, expect, it } from "vitest";
import { prepareVueJsxCarrier } from "./vueJsxAuthority";

const VUE_RUNTIME = `export namespace JSX {
  interface Element {}
  interface ElementClass { $props: {} }
  interface ElementAttributesProperty { $props: {} }
  interface IntrinsicElements {
    div: { class?: string }
    span: { class?: string }
  }
  interface IntrinsicAttributes {}
}
`;

const REACT_COMPETITOR = `export = React;
export as namespace React;
declare namespace React {
  namespace JSX {
    interface Element {}
    interface ElementChildrenAttribute { children: {} }
    interface IntrinsicElements {
      div: { className?: string; children?: never }
      span: { className?: string; children?: never }
    }
  }
}
`;

const roots: string[] = [];
const assetRoots: string[] = [];

function legacyOwnerAssetKey(root: string): string {
  const vue = realpathSync(path.join(root, "node_modules", "vue"));
  const runtime = realpathSync(path.join(vue, "jsx-runtime", "index.d.ts"));
  const fields: readonly (string | Buffer)[] = [
    "3.5.40",
    path.resolve(vue).replace(/\\/gu, "/"),
    path.resolve(runtime).replace(/\\/gu, "/"),
    readFileSync(path.join(vue, "package.json")),
    readFileSync(runtime),
  ];
  const hash = createHash("sha256");
  for (const field of fields) {
    const bytes = typeof field === "string" ? Buffer.from(field) : field;
    const length = Buffer.allocUnsafe(8);
    length.writeBigUInt64LE(BigInt(bytes.length));
    hash.update(length);
    hash.update(bytes);
  }
  return hash.digest("hex").slice(0, 24);
}

function workspace(): string {
  const root = path.join(
    tmpdir(),
    `verter-vue-jsx-authority-${process.pid}-${roots.length}-${Date.now()}`,
  );
  roots.push(root);
  const vue = path.join(root, "node_modules", "vue");
  mkdirSync(path.join(vue, "jsx-runtime"), { recursive: true });
  mkdirSync(path.join(root, "src"), { recursive: true });
  writeFileSync(
    path.join(vue, "package.json"),
    JSON.stringify({
      name: "vue",
      version: "3.5.40",
      exports: { "./jsx-runtime": { types: "./jsx-runtime/index.d.ts" } },
    }),
  );
  writeFileSync(path.join(vue, "jsx-runtime", "index.d.ts"), VUE_RUNTIME);
  writeFileSync(path.join(root, "react-shaped.d.ts"), REACT_COMPETITOR);
  return root;
}

function diagnostics(root: string, carrier: string, content: string, scriptKind: ts.ScriptKind) {
  const react = path.join(root, "react-shaped.d.ts");
  const key = (fileName: string) => path.resolve(fileName).replace(/\\/gu, "/").toLowerCase();
  const files = new Map<string, string>([
    [key(carrier), content],
    [key(react), REACT_COMPETITOR],
  ]);
  const host: ts.LanguageServiceHost = {
    getCompilationSettings: () => ({
      allowJs: scriptKind === ts.ScriptKind.JSX,
      checkJs: scriptKind === ts.ScriptKind.JSX,
      jsx: ts.JsxEmit.Preserve,
      module: ts.ModuleKind.ESNext,
      moduleResolution: ts.ModuleResolutionKind.Bundler,
      noEmit: true,
      strict: true,
      target: ts.ScriptTarget.ES2022,
    }),
    getCurrentDirectory: () => root,
    getDefaultLibFileName: (options) => ts.getDefaultLibFilePath(options),
    getScriptFileNames: () => [carrier, react],
    getScriptKind: (fileName) => (key(fileName) === key(carrier) ? scriptKind : ts.ScriptKind.TS),
    getScriptSnapshot: (fileName) => {
      const text = files.get(key(fileName)) ?? ts.sys.readFile(fileName);
      return text === undefined ? undefined : ts.ScriptSnapshot.fromString(text);
    },
    getScriptVersion: () => "0",
    fileExists: ts.sys.fileExists,
    readFile: ts.sys.readFile,
    readDirectory: ts.sys.readDirectory,
    directoryExists: ts.sys.directoryExists,
    getDirectories: ts.sys.getDirectories,
    realpath: ts.sys.realpath,
  };
  return ts.createLanguageService(host).getSemanticDiagnostics(carrier);
}

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
  for (const root of assetRoots.splice(0)) rmSync(root, { recursive: true, force: true });
});

describe("editor tsserver Vue JSX authority", () => {
  it.each([
    ["TypeScript", ".vue.tsx", ts.ScriptKind.TSX, ""],
    ["JavaScript + JSDoc", ".vue.jsx", ts.ScriptKind.JSX, "// @ts-check\n"],
  ])(
    "keeps valid nested class markup clean under a React competitor for %s carriers",
    (_label, suffix, scriptKind, checkDirective) => {
      const root = workspace();
      const carrier = path.join(root, "src", `App${suffix}`);
      const tail = `${checkDirective}const label = "ok";\nconst view = <div class="card"><span>{label}</span></div>;\n`;
      const original = `/** @jsxImportSource vue */\n${tail}`;

      const control = diagnostics(root, carrier, original, scriptKind);
      if (scriptKind === ts.ScriptKind.TSX) {
        expect(
          control.some((diagnostic) => diagnostic.code === 2322 || diagnostic.code === 2559),
        ).toBe(true);
      }

      const prepared = prepareVueJsxCarrier(carrier, original);
      expect(prepared).toBeDefined();
      expect(prepared!.content.split("\n").slice(1).join("\n")).toBe(tail);
      expect(prepared!.content.split("\n")).toHaveLength(original.split("\n").length);
      expect(prepared!.content).not.toContain("@jsxImportSource vue");
      expect(prepared!.adapterContent).toContain("interface ElementChildrenAttribute {}");

      const actual = diagnostics(root, carrier, prepared!.content, scriptKind);
      expect(
        actual.map(
          (diagnostic) =>
            `TS${diagnostic.code}: ${ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")}`,
        ),
      ).toEqual([]);

      const negative = prepareVueJsxCarrier(
        carrier,
        `/** @jsxImportSource vue */\n${checkDirective}declare const Child: new () => { $props: { label: string } };\nconst bad = <Child label={1} totallyFake />;\n`,
      );
      expect(negative).toBeDefined();
      const negativeDiagnostics = diagnostics(root, carrier, negative!.content, scriptKind);
      expect(
        negativeDiagnostics.some(
          (diagnostic) =>
            diagnostic.code === 2322 ||
            diagnostic.code === 2353 ||
            ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n").includes("totallyFake"),
        ),
        negativeDiagnostics
          .map(
            (diagnostic) =>
              `TS${diagnostic.code}: ${ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")}`,
          )
          .join("\n"),
      ).toBe(true);
    },
  );

  it("leaves non-Vue compiler intros and missing owner packages untouched", () => {
    const root = workspace();
    expect(
      prepareVueJsxCarrier(
        path.join(root, "src", "Widget.svelte.tsx"),
        "/** @jsxImportSource @verter/svelte-jsx */\nconst view = <div />;\n",
      ),
    ).toBeUndefined();
    expect(
      prepareVueJsxCarrier(
        path.join(tmpdir(), "no-vue", "App.vue.tsx"),
        "/** @jsxImportSource vue */\nconst view = <div />;\n",
      ),
    ).toBeUndefined();
  });

  it("does not let an immutable adapter from an earlier schema block current carriers", () => {
    const root = workspace();
    const carrier = path.join(root, "src", "SchemaUpgrade.vue.tsx");
    const legacyDirectory = path.join(
      tmpdir(),
      "verter-host",
      `vue-jsx-tsserver-${legacyOwnerAssetKey(root)}`,
    );
    assetRoots.push(legacyDirectory);
    mkdirSync(legacyDirectory, { recursive: true });
    const legacyPath = path.join(legacyDirectory, "classic.d.ts");
    writeFileSync(legacyPath, "// bytes from an earlier adapter schema\n");

    const prepared = prepareVueJsxCarrier(
      carrier,
      '/** @jsxImportSource vue */\nconst view = <div class="card" />;\n',
    );

    expect(prepared).toBeDefined();
    expect(prepared!.adapterPath).not.toBe(legacyPath);
    expect(readFileSync(prepared!.adapterPath, "utf8")).toBe(prepared!.adapterContent);
    assetRoots.push(path.dirname(prepared!.adapterPath));
  });
});
