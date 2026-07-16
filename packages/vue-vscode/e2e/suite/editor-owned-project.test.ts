import * as assert from "assert";
import * as fs from "fs";
import * as path from "path";

import * as vscode from "vscode";

import {
  FIXTURE_NAME,
  TYPE_PROVIDER,
  findPosition,
  hoverText,
  openVueFile,
  parseStartupTiming,
  readTestLog,
  waitForDiagnostics,
  waitForDiagnosticsSettled,
} from "../helpers";
import {
  descendantsOf,
  localSemanticEnginesUnderVerterLsp,
  readProcessInventory,
  type ProcessRecord,
} from "../processInventory";
import {
  parseArmedControlDir,
  sessionKeyFromControlDir,
  verifySharedArmedHandshake,
} from "../../src/sharedTsgoLaunch";

const FIXTURE = "editor-owned-project";
const REQUIRED_PROVIDERS = new Set(["tsserver", "shared-tsgo"]);

interface EditorTsserverReceiptObservation {
  pid: number;
  projects: string[];
}

interface RelayAdvertisement {
  advertisementVersion: number;
  protocol: number;
  endpoint: string;
  nonce: string;
  pid: number;
  sessionKey: string;
  realTsgo: string;
  realTsgoHash: number;
  wirePin: number;
  editorSessionGeneration: number;
}

if (FIXTURE_NAME === FIXTURE) {
  suite(`Editor-owned TypeScript project [${TYPE_PROVIDER ?? "unspecified"}]`, function () {
    this.timeout(60_000);

    let compDocument: vscode.TextDocument;
    let consumerDocument: vscode.TextDocument;
    let compDiagnostics: vscode.Diagnostic[];
    let consumerDiagnostics: vscode.Diagnostic[];

    suiteSetup(async function () {
      assert.ok(
        TYPE_PROVIDER && REQUIRED_PROVIDERS.has(TYPE_PROVIDER),
        `The ${FIXTURE} acceptance must run with tsserver or shared-tsgo, got ${TYPE_PROVIDER ?? "none"}`,
      );

      compDocument = await openVueFile("src/Comp.vue");
      await waitForDiagnostics(compDocument.uri, {
        timeoutMs: 30_000,
        predicate: (diagnostic) => diagnosticCode(diagnostic) === 2322,
      });
      compDiagnostics = await waitForDiagnosticsSettled(compDocument.uri, {
        timeoutMs: 10_000,
        stableMs: 800,
      });
      consumerDocument = await openVueFile("src/Consumer.ts");
      consumerDiagnostics = await waitForDiagnosticsSettled(consumerDocument.uri, {
        timeoutMs: 10_000,
        stableMs: 800,
      });
    });

    test("reports the attested editor route and exact configured project", function () {
      const timing = parseStartupTiming();
      const log = readTestLog();
      const expectedKind = TYPE_PROVIDER === "tsserver" ? "editor-tsserver" : "tsgo";

      assert.strictEqual(
        timing.providerKind,
        expectedKind,
        `Expected public provider status ${expectedKind}; reason=${timing.typeProviderReason ?? "none"}`,
      );
      assert.ok(timing.typeProviderReason, "The editor-owned route must publish its provenance");
      assert.doesNotMatch(
        log,
        /Type provider \((?:tsserver|tsgo)\) started with PID/,
        "Successful editor attachment must not report a managed semantic child",
      );

      const expectedTsconfig = path.join(workspaceRoot(), "tsconfig.json");
      if (TYPE_PROVIDER === "tsserver") {
        const observation = parseEditorTsserverObservation(log);
        assert.ok(
          observation.projects.some(
            (project) => normalizedPath(project) === normalizedPath(expectedTsconfig),
          ),
          `Editor tsserver receipt did not name the active project ${expectedTsconfig}: ${JSON.stringify(observation.projects)}`,
        );
        assert.match(
          timing.typeProviderReason!,
          new RegExp(`\\b${observation.pid}\\b`),
          "Public status must identify the attested editor process",
        );
      } else {
        assert.match(
          timing.typeProviderReason!,
          /editor-owned Native Preview/i,
          "Public status must identify Native Preview ownership",
        );
        const advertisement = readRelayAdvertisement(log);
        assert.ok(fs.existsSync(advertisement.realTsgo), advertisement.realTsgo);
        assert.ok(
          Number.isSafeInteger(advertisement.editorSessionGeneration) &&
            advertisement.editorSessionGeneration >= 0,
          "The relay advertisement must bind an editor session generation",
        );
      }
    });

    test("provides Vue JSX intrinsics without project jsxImportSource", async function () {
      const tsconfig = JSON.parse(
        fs.readFileSync(path.join(workspaceRoot(), "tsconfig.json"), "utf8"),
      ) as { compilerOptions?: { jsxImportSource?: unknown } };
      assert.strictEqual(
        tsconfig.compilerOptions?.jsxImportSource,
        undefined,
        "The acceptance must prove the carrier-owned JSX environment, not a fixture override",
      );

      const document = await openVueFile("src/App.vue");
      const diagnostics = await waitForDiagnosticsSettled(document.uri, {
        timeoutMs: 20_000,
        stableMs: 800,
      });
      const missingJsxEnvironment = diagnostics.filter(
        (diagnostic) =>
          diagnosticCode(diagnostic) === 7026 ||
          /JSX element implicitly has type 'any'|JSX\.IntrinsicElements/.test(diagnostic.message),
      );
      assert.deepStrictEqual(
        missingJsxEnvironment,
        [],
        `The ${TYPE_PROVIDER} editor route lacks Vue JSX intrinsics: ${formatDiagnostics(diagnostics)}`,
      );
    });

    test("maps configured-project diagnostics and exposes the real component surface", async function () {
      const ts2322 = compDiagnostics.filter((diagnostic) => diagnosticCode(diagnostic) === 2322);
      assert.strictEqual(
        ts2322.length,
        1,
        `Expected exactly one TS2322 from the deliberate assignment, got ${formatDiagnostics(compDiagnostics)}`,
      );
      const wrongLine = findPosition(compDocument, "const wrong: number = label")?.line;
      assert.notStrictEqual(wrongLine, undefined, "Fixture assignment must exist");
      assert.strictEqual(
        ts2322[0].range.start.line,
        wrongLine,
        "The diagnostic must map to the real .vue assignment line",
      );
      assert.ok(
        ts2322[0].range.end.isAfter(ts2322[0].range.start),
        "The mapped diagnostic must carry a non-empty source span",
      );
      assert.match(ts2322[0].message, /string.*number|number.*string/i);

      for (const diagnostic of compDiagnostics) {
        assert.notStrictEqual(
          diagnosticCode(diagnostic),
          2307,
          `The configured project must resolve carrier imports: ${diagnostic.message}`,
        );
        assert.doesNotMatch(diagnostic.message, /No Project/i);
      }
      assert.doesNotMatch(readTestLog(), /No Project/i);

      const labelPosition = findPosition(compDocument, "{{ label }}", 3);
      assert.ok(labelPosition, "Fixture label interpolation must exist");
      const labelHover = await waitForHover(compDocument.uri, labelPosition, (text) =>
        /\blabel\b/.test(text),
      );
      assertNoCarrierPath("carrier hover", labelHover.text);
      const mappedRange = labelHover.hovers.find((hover) => hover.range)?.range;
      if (mappedRange) {
        assert.strictEqual(mappedRange.start.line, labelPosition.line);
      }

      const consumerPosition = findPosition(
        consumerDocument,
        "export const comp = Comp",
        "export const comp = ".length,
      );
      assert.ok(consumerPosition, "Fixture component reference must exist");
      const consumerHover = await waitForHover(consumerDocument.uri, consumerPosition, (text) =>
        /\bLabelProps\b|\blabel\b|\bcount\b/.test(text),
      );
      assertNoCarrierPath("consumer hover", consumerHover.text);
      assert.doesNotMatch(
        consumerHover.text,
        /DefineComponent<\s*\{\s*\}\s*,\s*\{\s*\}>|:\s*any\b/,
        `The .ts -> .vue import degraded to an empty or any surface:\n${consumerHover.text}`,
      );
      assert.deepStrictEqual(
        consumerDiagnostics.filter(
          (diagnostic) => diagnostic.severity === vscode.DiagnosticSeverity.Error,
        ),
        [],
        `The structural component-surface checks must remain clean: ${formatDiagnostics(consumerDiagnostics)}`,
      );
    });

    test("keeps the editor semantic engine outside the Verter LSP process tree", async function () {
      const inventory = await readProcessInventory(10_000);
      const editorTree = descendantsOf(inventory, process.pid);
      const lspProcesses = editorTree.filter((row) => /^verter-lsp(?:\.exe)?$/i.test(row.name));
      assert.strictEqual(
        lspProcesses.length,
        1,
        `Expected one Verter LSP child, got ${formatProcesses(lspProcesses)}`,
      );
      assert.deepStrictEqual(
        localSemanticEnginesUnderVerterLsp(editorTree),
        [],
        "A successful editor attachment must leave no managed tsserver/tsgo below Verter LSP",
      );

      const lspDescendantPids = new Set(
        descendantsOf(editorTree, lspProcesses[0].pid).map((row) => row.pid),
      );
      const log = readTestLog();
      if (TYPE_PROVIDER === "tsserver") {
        const observation = parseEditorTsserverObservation(log);
        const process = inventory.find((row) => row.pid === observation.pid);
        assert.ok(process, `Attested editor tsserver ${observation.pid} is not running`);
        assert.match(
          process.commandLine,
          /(?:^|[\\/])tsserver(?:library)?\.js(?:["']|\s|$)/i,
          `Attested process is not tsserver: ${formatProcesses([process])}`,
        );
        assert.ok(!lspDescendantPids.has(observation.pid));
      } else {
        const advertisement = readRelayAdvertisement(log);
        assert.ok(
          editorTree.some((row) => row.pid === advertisement.pid),
          `Editor-spawned relay ${advertisement.pid} is not in the extension tree`,
        );
        const realEngineProcesses = editorTree.filter((row) =>
          commandMentionsPath(row, advertisement.realTsgo),
        );
        assert.strictEqual(
          realEngineProcesses.length,
          1,
          `Expected exactly one real Native Preview engine ${advertisement.realTsgo}, got ${formatProcesses(realEngineProcesses)}`,
        );
        assert.ok(
          descendantsOf(editorTree, advertisement.pid).some(
            (row) => row.pid === realEngineProcesses[0].pid,
          ),
          "The real engine must be owned by the editor-spawned relay",
        );
        assert.ok(!lspDescendantPids.has(realEngineProcesses[0].pid));
      }
    });
  });
}

function diagnosticCode(diagnostic: vscode.Diagnostic): string | number | undefined {
  const code = diagnostic.code;
  const value = code && typeof code === "object" ? code.value : code;
  // Native Preview currently publishes TypeScript codes as decimal strings,
  // while tsserver publishes numbers. Normalize the same TS diagnostic identity
  // so the two real editor-owned routes share one behavioral assertion.
  if (typeof value === "string" && /^\d+$/.test(value)) return Number(value);
  return value;
}

function workspaceRoot(): string {
  const folder = vscode.workspace.workspaceFolders?.[0];
  assert.ok(folder, "The acceptance fixture requires a workspace folder");
  return folder.uri.fsPath;
}

function normalizedPath(value: string): string {
  const normalized = path.resolve(value).replace(/\\/g, "/");
  return process.platform === "win32" ? normalized.toLowerCase() : normalized;
}

function parseEditorTsserverObservation(log: string): EditorTsserverReceiptObservation {
  const match = /\[editor-tsserver\] armed: pid=(\d+) projects=(\[[^\r\n]*\]) receipt=/.exec(log);
  assert.ok(match, `Missing editor-tsserver attestation in log:\n${log.slice(-4_000)}`);
  const projects = JSON.parse(match[2]) as unknown;
  assert.ok(Array.isArray(projects) && projects.every((entry) => typeof entry === "string"));
  return { pid: Number(match[1]), projects };
}

function readRelayAdvertisement(log: string): RelayAdvertisement {
  const controlDir = parseArmedControlDir(log);
  assert.ok(controlDir, `Missing Native Preview rendezvous in log:\n${log.slice(-4_000)}`);
  const entries = fs.readdirSync(controlDir);
  const verdict = verifySharedArmedHandshake({ logText: log, controlDirEntries: entries });
  assert.ok(verdict.ok, verdict.reason);
  assert.strictEqual(verdict.advertisements.length, 1);
  const raw = JSON.parse(
    fs.readFileSync(path.join(controlDir, verdict.advertisements[0]), "utf8"),
  ) as RelayAdvertisement;
  assert.strictEqual(raw.sessionKey, sessionKeyFromControlDir(controlDir));
  assert.ok(Number.isSafeInteger(raw.pid) && raw.pid > 0);
  assert.ok(typeof raw.realTsgo === "string" && raw.realTsgo.length > 0);
  assert.ok(typeof raw.endpoint === "string" && raw.endpoint.length > 0);
  return raw;
}

async function waitForHover(
  uri: vscode.Uri,
  position: vscode.Position,
  predicate: (text: string) => boolean,
  timeoutMs = 20_000,
): Promise<{ hovers: vscode.Hover[]; text: string }> {
  const started = Date.now();
  let latest = "";
  while (Date.now() - started < timeoutMs) {
    const hovers =
      (await vscode.commands.executeCommand<vscode.Hover[]>(
        "vscode.executeHoverProvider",
        uri,
        position,
      )) ?? [];
    latest = hovers.map(hoverText).join("\n");
    if (hovers.length > 0 && predicate(latest)) return { hovers, text: latest };
    await new Promise<void>((resolve) => setTimeout(resolve, 100));
  }
  assert.fail(`Timed out waiting for typed hover at ${uri.fsPath}:\n${latest}`);
}

function assertNoCarrierPath(channel: string, value: unknown): void {
  const serialized = typeof value === "string" ? value : JSON.stringify(value);
  assert.doesNotMatch(
    serialized,
    /(?:\.vue|\.svelte)\.(?:tsx?|jsx?)\b|verter-carrier-store/i,
    `${channel} leaked a generated carrier identity: ${serialized}`,
  );
}

function commandMentionsPath(process: ProcessRecord, expectedPath: string): boolean {
  return normalizedCommand(process.commandLine).includes(normalizedCommand(expectedPath));
}

function normalizedCommand(value: string): string {
  return value.replace(/\\/g, "/").toLowerCase();
}

function formatDiagnostics(diagnostics: readonly vscode.Diagnostic[]): string {
  return diagnostics
    .map(
      (diagnostic) =>
        `${diagnosticCode(diagnostic) ?? "none"}@${diagnostic.range.start.line}:${diagnostic.range.start.character} ${diagnostic.message}`,
    )
    .join(" | ");
}

function formatProcesses(processes: readonly ProcessRecord[]): string {
  return processes
    .map((process) => `${process.pid}<-${process.parentPid} ${process.name} ${process.commandLine}`)
    .join("\n");
}
