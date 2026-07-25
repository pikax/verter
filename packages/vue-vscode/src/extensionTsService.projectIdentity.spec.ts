// A configured project IS its config file — not the directory that file sits in.
//
// One directory routinely holds SEVERAL configured projects (`tsconfig.app.json`
// + `tsconfig.node.json` is the stock Vite layout), each with its own compiler
// options; and a project may be configured by `jsconfig.json`, `tsconfig.*.json`
// or a config in an ancestor directory. A consumer that reduces project identity
// to a directory, and then searches for the literal name `tsconfig.json` under
// it, therefore (a) collapses sibling projects onto ONE language service whose
// options are whichever project happened to open first, and (b) silently invents
// default compiler options for every project not named `tsconfig.json` —
// answering with diagnostics the user's own configuration does not describe.
//
// The LSP declares the owning config alongside the root
// (`crates/verter_lsp/src/extension_provider.rs`, proved in
// `extension_provider_tests.rs::open_declares_the_owning_projects_config_file_alongside_its_root`).
// These specs are the CONSUMER half: given that declaration, the registry keys
// services by it and the service parses THAT config.

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { ExtensionTsService } from "./extensionTsService.js";
import { materializeWorkspaceTypeScript } from "./extensionTsService.testUtils.js";
import { ExtensionTsServiceRegistry, createTsQueryHandler } from "./extensionTsServiceRegistry.js";

interface WireDiagnostic {
  code: number;
  text: string;
  category: string;
}

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

function makeTempRoot(name: string): string {
  const root = mkdtempSync(join(tmpdir(), name));
  tmps.push(root);
  return root;
}

/** `null` assigned to a `string` — an error under `strict`, fine without it. */
const STRICTNESS_PROBE = "export const probe: string = null;\n";

describe("extension TS provider — project identity is the config file", () => {
  it("gives two configured projects in ONE directory their own service and options", () => {
    // The stock Vite layout: one directory, two configured projects. They differ
    // in `strict`, so serving both from one service (or from one config) answers
    // one of them with the other's rules.
    const root = makeTempRoot("ext-ts-two-configs-");
    writeFileSync(join(root, "package.json"), JSON.stringify({ name: "fixture", private: true }));
    const strictConfig = join(root, "tsconfig.strict.json");
    const looseConfig = join(root, "tsconfig.loose.json");
    writeFileSync(
      strictConfig,
      JSON.stringify({
        compilerOptions: { module: "esnext", target: "esnext", strict: true },
        include: ["strictEntry.ts"],
      }),
    );
    writeFileSync(
      looseConfig,
      JSON.stringify({
        compilerOptions: { module: "esnext", target: "esnext", strict: false },
        include: ["looseEntry.ts"],
      }),
    );
    materializeWorkspaceTypeScript(root);

    const strictFile = join(root, "strictEntry.ts");
    const looseFile = join(root, "looseEntry.ts");
    writeFileSync(strictFile, STRICTNESS_PROBE);
    writeFileSync(looseFile, STRICTNESS_PROBE);

    const registry = new ExtensionTsServiceRegistry({});
    const handler = createTsQueryHandler(registry);

    for (const [file, config] of [
      [strictFile, strictConfig],
      [looseFile, looseConfig],
    ] as const) {
      handler({
        command: "open",
        arguments: {
          file,
          fileContent: STRICTNESS_PROBE,
          scriptKindName: "TS",
          projectRootPath: root,
          projectConfigPath: config,
        },
      });
    }

    const strictDiags = handler({
      command: "semanticDiagnosticsSync",
      arguments: { file: strictFile },
    }) as WireDiagnostic[];
    const looseDiags = handler({
      command: "semanticDiagnosticsSync",
      arguments: { file: looseFile },
    }) as WireDiagnostic[];

    expect(
      strictDiags.find((d) => d.code === 2322),
      `the strict project must report the null assignment: ${JSON.stringify(strictDiags)}`,
    ).toBeDefined();
    expect(
      looseDiags.find((d) => d.code === 2322),
      `the non-strict project must NOT: ${JSON.stringify(looseDiags)}`,
    ).toBeUndefined();
    // Same directory, two projects: two services, not one.
    expect(registry.projectRoots).toEqual([root, root]);
  });

  it("honours a declared jsconfig.json instead of inventing default options", () => {
    // `jsconfig.json` configures a project exactly like `tsconfig.json`. Searching
    // for the literal name `tsconfig.json` finds nothing here, and the invented
    // fallback options carry `checkJs: false` — so the project's own opt-in to JS
    // type-checking is silently dropped and the file reports clean.
    const root = makeTempRoot("ext-ts-jsconfig-");
    writeFileSync(join(root, "package.json"), JSON.stringify({ name: "fixture", private: true }));
    const configPath = join(root, "jsconfig.json");
    writeFileSync(
      configPath,
      JSON.stringify({
        compilerOptions: { module: "esnext", target: "esnext", allowJs: true, checkJs: true },
        include: ["*.js"],
      }),
    );
    materializeWorkspaceTypeScript(root);

    const source = "/** @type {number} */\nexport const n = 'not a number';\n";
    const filePath = join(root, "entry.js");
    writeFileSync(filePath, source);

    const svc = new ExtensionTsService(root, undefined, configPath);
    svc.handleQuery("open", { file: filePath, fileContent: source, scriptKindName: "JS" });
    const diags = svc.handleQuery("semanticDiagnosticsSync", {
      file: filePath,
    }) as WireDiagnostic[];

    expect(
      diags.find((d) => d.code === 2322),
      `checkJs is ON in this project's own config, so the annotated mismatch must be ` +
        `reported: ${JSON.stringify(diags)}`,
    ).toBeDefined();
  });

  it("parses a discovered ancestor config against the CONFIG's directory", () => {
    // No config at the project root, one in the ancestor — the walk-up case. Its
    // `baseUrl`/`paths` are relative to the CONFIG, so parsing them against the
    // project root instead points every alias at a directory that does not exist
    // and every aliased import reports "cannot find module".
    const parent = makeTempRoot("ext-ts-ancestor-config-");
    writeFileSync(
      join(parent, "tsconfig.json"),
      JSON.stringify({
        compilerOptions: {
          module: "esnext",
          target: "esnext",
          moduleResolution: "bundler",
          baseUrl: ".",
          paths: { "@lib/*": ["lib/*"] },
        },
      }),
    );
    mkdirSync(join(parent, "lib"), { recursive: true });
    writeFileSync(join(parent, "lib", "thing.ts"), "export const thing = 1;\n");

    const projectRoot = join(parent, "app");
    mkdirSync(projectRoot, { recursive: true });
    writeFileSync(
      join(projectRoot, "package.json"),
      JSON.stringify({ name: "fixture", private: true }),
    );
    materializeWorkspaceTypeScript(projectRoot);

    const source = 'import { thing } from "@lib/thing";\nexport const v: number = thing;\n';
    const filePath = join(projectRoot, "entry.ts");
    writeFileSync(filePath, source);

    const svc = new ExtensionTsService(projectRoot);
    svc.handleQuery("open", { file: filePath, fileContent: source, scriptKindName: "TS" });
    const diags = svc.handleQuery("semanticDiagnosticsSync", {
      file: filePath,
    }) as WireDiagnostic[];

    expect(
      diags.find((d) => d.code === 2307),
      `"@lib/*" resolves relative to the config that declares it, so the import must ` +
        `resolve: ${JSON.stringify(diags)}`,
    ).toBeUndefined();
  });
});

// A config that EXISTS but cannot be consumed must fail closed.
//
// Restoring the invented defaults there is the worst available outcome: the
// project HAS rules, the service could not read them, and the user is answered
// under a different rule set (`strict: true`, `checkJs: false`, `jsx: react-jsx`,
// no path aliases) with nothing anywhere saying their configuration was
// discarded. Every assertion below therefore demands the REFUSAL, and the last
// one demands that a merely input-less config still serves — the fail-closed
// rule must not become "any config diagnostic disables the project".
describe("extension TS provider — an unusable declared config fails closed", () => {
  /** A project root with a real TypeScript and the given config text. */
  function makeProjectWithConfigText(name: string, configText: string): [string, string, string] {
    const root = makeTempRoot(name);
    writeFileSync(join(root, "package.json"), JSON.stringify({ name: "fixture", private: true }));
    const configPath = join(root, "tsconfig.json");
    writeFileSync(configPath, configText);
    materializeWorkspaceTypeScript(root);
    const filePath = join(root, "entry.ts");
    writeFileSync(filePath, STRICTNESS_PROBE);
    return [root, configPath, filePath];
  }

  it("refuses a declared config whose JSON is malformed", () => {
    // Under the defaults this file type-checks under `strict: true` and the
    // service answers as if nothing were wrong — a project silently governed by
    // rules its author never wrote.
    const [root, configPath, filePath] = makeProjectWithConfigText(
      "ext-ts-broken-config-",
      '{ "compilerOptions": { "strict": false ',
    );

    const notified: string[] = [];
    const svc = new ExtensionTsService(root, (message) => notified.push(message), configPath);

    expect(() =>
      svc.handleQuery("open", {
        file: filePath,
        fileContent: STRICTNESS_PROBE,
        scriptKindName: "TS",
      }),
    ).toThrow(/could not read the configuration file that defines this project/);
    // The refusal is cached and actionable, exactly like the missing-TypeScript one.
    expect(notified).toHaveLength(1);
    expect(notified[0]).toContain(configPath);
    expect(() => svc.handleQuery("semanticDiagnosticsSync", { file: filePath })).toThrow(
      /could not read the configuration file that defines this project/,
    );
  });

  it("refuses a declared config that is not on disk", () => {
    const root = makeTempRoot("ext-ts-missing-config-");
    writeFileSync(join(root, "package.json"), JSON.stringify({ name: "fixture", private: true }));
    materializeWorkspaceTypeScript(root);
    const filePath = join(root, "entry.ts");
    writeFileSync(filePath, STRICTNESS_PROBE);

    const svc = new ExtensionTsService(
      root,
      undefined,
      // The LSP declared this project's identity; the file is gone (deleted
      // between the workspace walk and the open). A project whose defining
      // config cannot be read is not a project this service may invent options for.
      join(root, "tsconfig.app.json"),
    );

    expect(() =>
      svc.handleQuery("open", {
        file: filePath,
        fileContent: STRICTNESS_PROBE,
        scriptKindName: "TS",
      }),
    ).toThrow(/could not read the configuration file that defines this project/);
  });

  it("refuses a declared config TypeScript could only partially salvage", () => {
    // `readConfigFile` succeeds — the JSON is well-formed — but
    // `parseJsonConfigFileContent` reports the option as invalid and returns
    // options with that field dropped. Serving those is serving rules the author
    // did not write, so the errors are not ignorable.
    const [root, configPath, filePath] = makeProjectWithConfigText(
      "ext-ts-invalid-option-",
      JSON.stringify({ compilerOptions: { target: "esnext", module: "not-a-module-kind" } }),
    );

    const svc = new ExtensionTsService(root, undefined, configPath);

    expect(() =>
      svc.handleQuery("open", {
        file: filePath,
        fileContent: STRICTNESS_PROBE,
        scriptKindName: "TS",
      }),
    ).toThrow(/could not parse the configuration file that defines this project/);
  });

  it("still serves a valid config that matches no input files", () => {
    // TS18003 ("No inputs were found") reports on the CONFIG's own file list.
    // This service's program is the set of files the LSP opens — a solution-style
    // `files: []` config, or a package whose sources are all carriers, is
    // completely normal and its options parsed fine. Refusing here would disable
    // the provider for correctly-configured projects.
    const [root, configPath, filePath] = makeProjectWithConfigText(
      "ext-ts-no-inputs-",
      JSON.stringify({
        compilerOptions: { module: "esnext", target: "esnext", strict: true },
        include: ["nothing-matches-this/**/*"],
      }),
    );

    const svc = new ExtensionTsService(root, undefined, configPath);
    svc.handleQuery("open", {
      file: filePath,
      fileContent: STRICTNESS_PROBE,
      scriptKindName: "TS",
    });
    const diags = svc.handleQuery("semanticDiagnosticsSync", {
      file: filePath,
    }) as WireDiagnostic[];

    expect(
      diags.find((d) => d.code === 2322),
      `the project's own \`strict: true\` must still be in force: ${JSON.stringify(diags)}`,
    ).toBeDefined();
  });
});
