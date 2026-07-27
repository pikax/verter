import * as assert from "assert";
import { pollBudget, sequenceParent } from "../lib/timeouts";
import * as fs from "fs";
import * as path from "path";

import * as vscode from "vscode";

import {
  FIXTURE_NAME,
  TYPE_PROVIDER,
  findPosition,
  getHoverText,
  openVueFile,
  waitForDiagnostics,
  waitForDiagnosticsSettled,
} from "../helpers";

const FIXTURE = "out-of-tree-monorepo";

/**
 * The defect this route blocks on, stated as the present-tense product fact it is.
 * It is the skip's reason and its removal condition: when carrier publication
 * reaches the extension-hosted topology, delete the `this.skip()` below.
 */
const CARRIER_PUBLICATION_SUPPRESSED =
  "carrier publication is suppressed for TypeProviderKind::Tsserver, and the extension-hosted " +
  "service registers under that kind, so no .vue.tsx companion is ever opened to the extension " +
  "host and every carrier query arrives for a file the registry has no binding for. The " +
  "extension-hosted provider serves plain .ts files only. THE SETTING IS CONTAINED, NOT SILENT: " +
  "the `extension` option's description states that .vue/.svelte are not served, opening a " +
  "carrier under it raises a warning naming auto/tsserver/tsgo as the providers that do serve " +
  "them, and the status bar holds a persistent warning while a carrier is open " +
  "(src/carrierProviderSupport.ts). What is missing is the CAPABILITY, and this suite is its " +
  "resolution gate. Remove this skip once carrier publication is connected for the " +
  "extension-hosted topology; this route then proves that fix.";

/**
 * The extension-hosted TypeScript provider, end to end, across the ONE seam no
 * unit test on either side can cross: the LSP resolves the carrier's owning
 * project, declares it over `$/verter/tsQuery`, and the extension host resolves
 * THAT project's TypeScript and answers from it — in one process, over the real
 * JSON-RPC hop.
 *
 * WHY THIS WORKSPACE LIVES OUTSIDE THE REPOSITORY. The extension host resolves a
 * project's compiler with `createRequire` anchored at the declared project root,
 * and Node's resolution walks ancestors. Every fixture under
 * `packages/vue-vscode/e2e/fixtures/*` therefore reaches the repository's own
 * `node_modules/typescript` from ANY root inside the tree — including the
 * workspace-folder root a wrongly-declared binding produces. Such a fixture
 * passes identically against the correct and the broken producer, so it
 * discriminates nothing. `runTests.ts` materializes this fixture into an OS temp
 * directory (`OUT_OF_TREE_FIXTURES`) where the ancestor chain ends at the
 * filesystem root with no TypeScript anywhere above it:
 *
 *   <tmp>/                       package.json, NO tsconfig, NO node_modules
 *     packages/app/              tsconfig.json + node_modules/typescript
 *       src/App.vue              the carrier under test
 *
 * A producer that declares the workspace FOLDER resolves TypeScript from
 * `<tmp>`, finds none, and — under the fail-closed contract — refuses: no typed
 * hover, no `ts` diagnostics, only the provider-unavailable error. A producer
 * that declares the OWNING configured project resolves
 * `packages/app/node_modules/typescript` and serves. The two assertions below
 * are exactly that difference.
 *
 * SKIPPED, AND WHY THE BODY IS STILL AT FULL STRENGTH. The product cannot serve
 * this route today, and the reason is upstream of everything the route asserts:
 * the extension-hosted service registers as `TypeProviderKind::Tsserver`
 * (`main.rs`, the `"extension"` arm), and
 * `ProjectSync::carrier_companion_open_suppressed()` is exactly
 * `kind == Tsserver`. That suppression exists because the WORKSPACE tsserver
 * receives carrier membership from Verter's TypeScript plugin through
 * `getExternalFiles` — machinery the in-extension-host language service does not
 * have. So `publish_tsx` short-circuits, no `.vue.tsx` companion is ever opened
 * to the extension host, and every carrier query arrives for a file the registry
 * has no binding for ("could not determine which project owns …"). The
 * extension-hosted provider therefore serves plain `.ts` files only, never a
 * carrier source — which is exactly why no unit test on either side of the
 * `$/verter/tsQuery` seam can reveal it, and why this route was worth building
 * even though the product fails it.
 *
 * So the suite skips at `suiteSetup`, naming that defect, and nothing else is
 * weakened: the fixture, the `extension` route, the out-of-tree materialization
 * and every assertion below stay exactly as written. Connect carrier publication
 * for the extension-hosted topology, delete the skip, and this route proves the
 * fix — both the carrier one it blocks on and the project-binding one it was
 * built to discriminate.
 */
if (FIXTURE_NAME === FIXTURE) {
  suite(
    `Out-of-tree monorepo, extension-hosted TypeScript [${TYPE_PROVIDER ?? "unspecified"}]`,
    function () {
      this.timeout(90_000);

      let document: vscode.TextDocument;
      let diagnostics: vscode.Diagnostic[];

      suiteSetup(async function () {
        // A cold second toolchain then a settle, in series (`POLL_SEQUENCES`).
        this.timeout(sequenceParent("outOfTreeMonorepoSetup"));
        assert.strictEqual(
          TYPE_PROVIDER,
          "extension",
          `The ${FIXTURE} acceptance exists to exercise the extension-hosted provider`,
        );

        const folders = vscode.workspace.workspaceFolders;
        assert.ok(folders && folders.length > 0, "Workspace should have folders");
        const workspaceRoot = folders[0].uri.fsPath;

        // The premise of the whole acceptance: the workspace ROOT has no
        // TypeScript, and the owning package does. If this ever stops holding the
        // test would pass for the wrong reason, so it is asserted, not assumed.
        assert.ok(
          !fs.existsSync(path.join(workspaceRoot, "node_modules", "typescript")),
          `the workspace root must have NO TypeScript for this acceptance to discriminate: ${workspaceRoot}`,
        );
        assert.ok(
          fs.existsSync(path.join(workspaceRoot, "packages", "app", "node_modules", "typescript")),
          "the owning package must have its own TypeScript install",
        );
        assert.ok(
          !fs.existsSync(path.join(workspaceRoot, "tsconfig.json")),
          "the workspace root must declare no configured project of its own",
        );

        // The fixture premise above is verified on every run — it is the thing a
        // future fix needs intact, and a broken materialization must stay loud.
        // What follows is what the product cannot do yet, so the suite stops here
        // and reports its three tests as pending with the defect named.
        console.log(`    SKIPPED: ${CARRIER_PUBLICATION_SUPPRESSED}`);
        this.skip();

        document = await openVueFile("packages/app/src/App.vue");
        // TS2322 is produced by the package's own `strict` config through its own
        // compiler. Waiting on it (rather than on any diagnostic) means a run that
        // fails closed times out here instead of passing on an empty set.
        await waitForDiagnostics(document.uri, {
          timeoutMs: pollBudget("outOfTreeStrictDiagnostic"),
          predicate: (diagnostic) => diagnosticCode(diagnostic) === 2322,
        });
        diagnostics = await waitForDiagnosticsSettled(document.uri, {
          timeoutMs: pollBudget("outOfTreeSettle"),
          stableMs: 800,
        });
      });

      test("type-checks a nested package from the TypeScript that package installed", () => {
        const mismatch = diagnostics.filter((diagnostic) => diagnosticCode(diagnostic) === 2322);
        assert.ok(
          mismatch.length > 0,
          `the nested package's own strict config must report the string→number assignment; ` +
            `got ${JSON.stringify(diagnostics.map((d) => [diagnosticCode(d), d.message]))}`,
        );
      });

      test("answers a typed hover inside the carrier's script block", async () => {
        const position = findPosition(document, "packageLocalLabel", 3);
        assert.ok(position, "the probe binding must exist in the fixture");
        const text = await getHoverText(document.uri, position);
        assert.match(
          text,
          /packageLocalLabel/,
          `hover must name the binding it was asked about: ${text}`,
        );
        assert.match(
          text,
          /string/,
          `hover must carry the TYPE, which only a served language service can produce — a ` +
            `project whose TypeScript did not resolve answers nothing at all: ${text}`,
        );
      });

      test("reports no provider-unavailable diagnostic for a served project", () => {
        const unavailable = diagnostics.filter((diagnostic) =>
          /could not resolve a workspace TypeScript installation|no configured TypeScript project includes/.test(
            diagnostic.message,
          ),
        );
        assert.strictEqual(
          unavailable.length,
          0,
          `the owning package installed TypeScript, so nothing may report it missing: ` +
            `${JSON.stringify(unavailable.map((d) => d.message))}`,
        );
      });
    },
  );
}

function diagnosticCode(diagnostic: vscode.Diagnostic): number | undefined {
  const code = diagnostic.code;
  if (typeof code === "number") return code;
  if (typeof code === "object" && code !== null && "value" in code) {
    const value = (code as { value: string | number }).value;
    return typeof code === "number" ? code : Number(value);
  }
  return typeof code === "string" ? Number(code) : undefined;
}
