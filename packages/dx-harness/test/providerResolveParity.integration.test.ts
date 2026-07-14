// Real-provider auto-import-on-accept PARITY against the REAL tsgo AND tsserver.
//
// Issue #1's headline guarantee: completion auto-import-on-accept now works under
// EVERY provider, not just tsgo. This is the discriminating, real-spawn proof —
// it drives the actual `verter-dx-baseline` bridge against BOTH a real tsgo and a
// real tsserver, runs a `completion` query, picks the item carrying the
// provider-pure resolve handle, re-issues `completionItem/resolve` through the
// bridge's `resolveCompletion` route, and asserts BOTH providers return the SAME
// auto-import `additionalTextEdits`. Before the fix, only tsgo resolved an import
// edit; tsserver returned nothing (the `data: None` discard + missing
// `resolve_completion`). A regression that re-breaks either provider's resolve
// makes this go RED.
//
//   cargo build -p verter_dx_baseline      # target/debug/verter-dx-baseline
//   DX_BASELINE_BIN=$PWD/target/debug/verter-dx-baseline \
//     pnpm -C packages/dx-harness test providerResolveParity
//
// Require-mode: set DX_REQUIRE_PROVIDERS=1 to make a provider skip (binary
// missing / spawn failed) a HARD FAILURE instead of a silent pass — the CI gate
// uses this so the parity proof can never vacuously skip when the assets are
// present (review finding T2).
import { existsSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { afterEach, describe, expect, it } from "vitest";

import {
  BridgeClient,
  type CompletionResolveData,
  type NormalizedCompletionItem,
  type ProviderName,
} from "../src/baseline/bridgeClient.js";
import { canonicalizePath } from "../src/paths.js";
import { resolveToolRoots } from "../src/toolRoots.js";

const BASELINE_BIN = process.env.DX_BASELINE_BIN;
const REQUIRE_PROVIDERS = process.env.DX_REQUIRE_PROVIDERS === "1";

const tmps: string[] = [];
const bridges: BridgeClient[] = [];
afterEach(async () => {
  for (const b of bridges.splice(0)) await b.dispose();
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

/** A bound import the resolved edit must produce: `import { myHelper } from "./helper"`. */
const EXPECT_IMPORT_SYMBOL = "myHelper";
const EXPECT_IMPORT_MODULE = "./helper";

/**
 * Walk up from `start` to the monorepo root — the directory holding
 * `pnpm-workspace.yaml`. vitest runs with `cwd` set to the PACKAGE dir
 * (`packages/dx-harness`), so the tool-root resolver (which composes
 * `<repoRoot>/packages/vue-vscode/...`) needs the real repo root, not the cwd.
 */
function findRepoRoot(start: string): string {
  let dir = start;
  for (let i = 0; i < 12; i++) {
    if (existsSync(join(dir, "pnpm-workspace.yaml"))) return dir;
    const parent = join(dir, "..");
    if (parent === dir) break;
    dir = parent;
  }
  return start;
}

/**
 * Locate the vendored `@typescript/native-preview` `tsgo` binary in the pnpm
 * store, so the tsgo provider runs without depending on a `tsgo` on PATH. Returns
 * the canonical path, or `undefined` when the platform package is not installed.
 */
function findVendoredTsgo(repoRoot: string): string | undefined {
  const pnpmDir = join(repoRoot, "node_modules", ".pnpm");
  if (!existsSync(pnpmDir)) return undefined;
  const platformDir = readdirSync(pnpmDir).find((d) => d.startsWith("@typescript+native-preview-"));
  if (!platformDir) return undefined;
  const scope = platformDir.slice("@typescript+".length).split("@")[0]; // native-preview-win32-x64
  const exe = process.platform === "win32" ? "tsgo.exe" : "tsgo";
  const candidate = join(pnpmDir, platformDir, "node_modules", "@typescript", scope, "lib", exe);
  return existsSync(candidate) ? canonicalizePath(candidate) : undefined;
}

/** Whether a resolve handle is the auto-import-capable shape (mirrors `is_actionable`). */
function isActionable(data: CompletionResolveData | undefined): boolean {
  if (!data) return false;
  if (data.kind === "lsp") return true;
  return data.source !== undefined || data.data !== undefined;
}

interface ResolvedParity {
  /** The single resolved import edit's text (`import { myHelper } from "./helper";`). */
  importEdit: string;
  /** Whether the provider was actually spawned (vs skipped — binary absent). */
  ran: boolean;
}

/**
 * Drive one real provider through the bridge: open a tiny two-file workspace where
 * `entry.ts` references an UNIMPORTED `myHelper` exported by `helper.ts`, request
 * completion at that reference, find the auto-import item, resolve it, and return
 * the single import edit it produced.
 */
async function resolveAutoImportFor(
  provider: ProviderName,
  repoRoot: string,
): Promise<ResolvedParity> {
  const root = mkdtempSync(join(tmpdir(), `dx-parity-${provider}-`));
  tmps.push(root);
  // A tsconfig makes both files one TS program, so the provider's
  // `includeExternalModuleExports` completion surfaces `helper.ts`'s export as an
  // auto-import candidate for the unimported reference in `entry.ts`.
  writeFileSync(
    join(root, "tsconfig.json"),
    JSON.stringify({
      compilerOptions: { module: "esnext", target: "esnext", moduleResolution: "bundler" },
      include: ["*.ts"],
    }),
  );
  const helperPath = join(root, "helper.ts");
  const entryPath = join(root, "entry.ts");
  writeFileSync(helperPath, "export const myHelper = 41;\n");
  // `myHelper` is referenced but NOT imported — both providers offer it as an
  // auto-import from `./helper`. The reference sits at the end of line 0.
  const entrySource = "myHelper\n";
  writeFileSync(entryPath, entrySource);

  const tools = resolveToolRoots(repoRoot, { tsgoBin: findVendoredTsgo(repoRoot) });
  if (process.env.DX_PARITY_DEBUG) {
    console.error(`[${provider}] toolRoot=${JSON.stringify(tools)} repoRoot=${repoRoot}`);
  }

  const bridge = new BridgeClient(BASELINE_BIN!, { requestTimeoutMs: 30_000 });
  bridges.push(bridge);
  const hello = await bridge.hello({
    workspaceRoot: root,
    repoRoot,
    provider,
    strictCi: false,
    toolRoot: {
      tsserverTsdk: tools.tsserverTsdk,
      expectedTsserverJs: tools.expectedTsserverJs,
      tsserverVersion: tools.tsserverVersion,
      tsgoBin: tools.tsgoBin,
    },
  });
  if (hello.type !== "hello") {
    throw new Error(`expected hello, got ${hello.type}: ${JSON.stringify(hello)}`);
  }
  if (hello.skipped) {
    // Provider binary absent / spawn failed — record the reason, do not assert.
    if (process.env.DX_PARITY_DEBUG) {
      console.error(`[${provider}] SKIPPED: ${hello.skipReason}`);
    }
    expect(typeof hello.skipReason).toBe("string");
    return { importEdit: "", ran: false };
  }

  const entryUri = pathToFileURL(entryPath).toString();
  await bridge.open(
    [
      { path: entryPath, content: entrySource, role: "entry" },
      { path: helperPath, content: "export const myHelper = 41;\n", role: "support" },
    ],
    1,
  );

  // Completion at the END of `myHelper` (byte offset 8 = length of "myHelper").
  const completion = await bridge.query({
    method: "completion",
    uri: entryUri,
    path: entryPath,
    offset: EXPECT_IMPORT_SYMBOL.length,
    version: 1,
  });
  if (completion.type !== "query" || completion.result.kind !== "completion") {
    throw new Error(`expected a completion query result, got ${JSON.stringify(completion)}`);
  }
  if (process.env.DX_PARITY_DEBUG) {
    const myHelperItems = completion.result.items.filter((i) => i.label === EXPECT_IMPORT_SYMBOL);
    console.error(
      `[${provider}] total items=${completion.result.items.length} myHelper items=${JSON.stringify(myHelperItems)}`,
    );
  }

  // The auto-import item: label `myHelper`, carrying an ACTIONABLE resolve handle.
  const item: NormalizedCompletionItem | undefined = completion.result.items.find(
    (i) => i.label === EXPECT_IMPORT_SYMBOL && isActionable(i.resolveData),
  );
  expect(
    item,
    `${provider} must offer an actionable auto-import completion for ${EXPECT_IMPORT_SYMBOL}`,
  ).toBeDefined();
  const resolveData = item!.resolveData!;

  // Re-issue resolve through the bridge — the SAME real provider resolves the edit.
  const resolved = await bridge.resolveCompletion({
    uri: entryUri,
    path: entryPath,
    version: 1,
    data: resolveData,
  });
  if (resolved.type !== "resolveCompletion") {
    throw new Error(`expected resolveCompletion, got ${JSON.stringify(resolved)}`);
  }
  expect(
    resolved.additionalTextEdits.length,
    `${provider} resolve must return an auto-import edit`,
  ).toBeGreaterThan(0);

  // Concatenate every edit's text (the import insertion is a single edit in practice).
  const importText = resolved.additionalTextEdits.map((e) => e.newText).join("");
  if (process.env.DX_PARITY_DEBUG) {
    console.error(`[${provider}] resolved import edit = ${JSON.stringify(importText)}`);
  }
  return { importEdit: importText, ran: true };
}

describe.skipIf(!BASELINE_BIN)("provider auto-import parity — real tsgo and tsserver agree", () => {
  it("tsgo and tsserver resolve the SAME auto-import edit for an unimported symbol", async () => {
    const repoRoot = canonicalizePath(findRepoRoot(process.cwd()));

    const tsgo = await resolveAutoImportFor("tsgo", repoRoot);
    const tsserver = await resolveAutoImportFor("tsserver", repoRoot);

    if (REQUIRE_PROVIDERS) {
      expect(tsgo.ran, "DX_REQUIRE_PROVIDERS=1: tsgo must actually run").toBe(true);
      expect(tsserver.ran, "DX_REQUIRE_PROVIDERS=1: tsserver must actually run").toBe(true);
    }
    if (!tsgo.ran || !tsserver.ran) {
      // At least one provider is unavailable in this environment and require-mode
      // is off — nothing to compare (the assertions inside each helper already ran
      // for whichever provider DID spawn).
      return;
    }

    // Each provider produced a real auto-import for `myHelper` from `./helper`.
    for (const [name, r] of [
      ["tsgo", tsgo],
      ["tsserver", tsserver],
    ] as const) {
      expect(r.importEdit, `${name} import edit imports the symbol`).toContain(
        EXPECT_IMPORT_SYMBOL,
      );
      expect(r.importEdit, `${name} import edit references the module`).toContain(
        EXPECT_IMPORT_MODULE,
      );
    }

    // PARITY: the two providers produce the SAME import edit text. Normalize only
    // insignificant trailing whitespace/quotes differences are NOT expected — both
    // are TypeScript's own quick-fix output, so the inserted import should match.
    const normalize = (s: string): string => s.replace(/\r\n/g, "\n").trim();
    expect(normalize(tsserver.importEdit)).toBe(normalize(tsgo.importEdit));
  });
});
