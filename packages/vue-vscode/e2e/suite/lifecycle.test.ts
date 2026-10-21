import * as assert from "assert";
import * as vscode from "vscode";
import { pollBudget, sequenceParent } from "../lib/timeouts";
import { descendantsOf, readProcessInventory, type ProcessRecord } from "../processInventory";
import {
  FIXTURE_NAME,
  ensureFixtureWarm,
  getAppVuePath,
  invalidateTypeProviderSyncCache,
  logMark,
  openVueFile,
  readTestLog,
  sleep,
  waitForDiagnostics,
  waitForDiagnosticsSettled,
} from "../helpers";

/**
 * The language server's lifecycle in the real extension host.
 *
 * This window owns exactly one Verter LSP process. A restart replaces it and a
 * crash is recovered by the extension's bounded restart policy alone — the
 * language client's own restart-on-close is switched off — so neither stacks a
 * second server, leaves the old one running, or doubles a document's
 * diagnostics. An unsaved edit survives both: the replacement server is synced
 * from the editor's buffer and reports on it.
 *
 * The assertions read the OS process table under this extension host and the
 * diagnostics VS Code holds, not the extension's own bookkeeping.
 */
suite(`Language server lifecycle [${FIXTURE_NAME}]`, function () {
  // Line 1 of the entry, right after `<script setup lang="ts">`: one TS2322 the
  // server reports only from the editor's unsaved buffer.
  const PROBE = 'const verterLifecycleProbe: number = "unsaved edit";\n';
  const PROBE_LINE = 1;
  let doc: vscode.TextDocument;

  const isProbe = (diagnostic: vscode.Diagnostic) => diagnostic.range.start.line === PROBE_LINE;
  const probeDiagnostics = () => vscode.languages.getDiagnostics(doc.uri).filter(isProbe);

  async function verterLspProcesses(): Promise<ProcessRecord[]> {
    const inventory = await readProcessInventory(10_000);
    return descendantsOf(inventory, process.pid).filter((row) =>
      /^verter-lsp(?:\.exe)?$/i.test(row.name),
    );
  }

  /**
   * Wait until exactly one Verter LSP runs under this extension host, it is
   * none of `replaced`, and it announced readiness after `mark`.
   */
  async function waitForReplacementServer(
    replaced: ReadonlySet<number>,
    mark: number,
  ): Promise<ProcessRecord> {
    const deadline = Date.now() + pollBudget("lifecycleServerReplaced");
    let running: ProcessRecord[] = [];
    while (Date.now() < deadline) {
      running = await verterLspProcesses();
      if (
        running.length === 1 &&
        !replaced.has(running[0].pid) &&
        readTestLog().slice(mark).includes("Verter ready")
      ) {
        invalidateTypeProviderSyncCache(mark);
        return running[0];
      }
      await sleep(250);
    }
    assert.fail(
      `expected one ready replacement Verter LSP (replaced: ${[...replaced].join(", ")}); ` +
        `running now: ${running.map((row) => row.pid).join(", ") || "none"}`,
    );
  }

  async function waitForProbeDiagnostic(): Promise<number> {
    await waitForDiagnostics(doc.uri, { predicate: isProbe, minCount: 1 });
    await waitForDiagnosticsSettled(doc.uri);
    return probeDiagnostics().length;
  }

  suiteSetup(async function () {
    await ensureFixtureWarm();
    doc = await openVueFile(getAppVuePath());
    assert.ok(
      doc.lineAt(0).text.startsWith('<script setup lang="ts">'),
      "the entry must open with its TypeScript setup script",
    );
    const edit = new vscode.WorkspaceEdit();
    edit.insert(doc.uri, new vscode.Position(PROBE_LINE, 0), PROBE);
    assert.ok(await vscode.workspace.applyEdit(edit), "the unsaved probe edit must apply");
  });

  suiteTeardown(async function () {
    if (!doc) return;
    await vscode.window.showTextDocument(doc, { preview: false });
    await vscode.commands.executeCommand("workbench.action.files.revert");
  });

  test("restart loops replace the one server and never duplicate diagnostics", async function () {
    this.timeout(sequenceParent("lifecycleRestartLoop"));
    const before = await waitForProbeDiagnostic();
    assert.ok(before >= 1, "the unsaved edit must be diagnosed before any restart");
    const initial = await verterLspProcesses();
    assert.strictEqual(initial.length, 1, "one Verter LSP before the restarts");

    const seen = new Set(initial.map((row) => row.pid));
    for (let restart = 0; restart < 3; restart += 1) {
      const mark = logMark();
      await vscode.commands.executeCommand("verter.restartLanguageServer");
      seen.add((await waitForReplacementServer(seen, mark)).pid);
    }

    assert.strictEqual(
      await waitForProbeDiagnostic(),
      before,
      "restarts must not register a second diagnostics source for the document",
    );
  });

  test("a crashed server is replaced once by the bounded recovery and keeps the unsaved edit", async function () {
    this.timeout(sequenceParent("lifecycleCrashRecovery"));
    const before = await waitForProbeDiagnostic();
    const [crashed, ...others] = await verterLspProcesses();
    assert.ok(crashed, "a Verter LSP must be running before the crash");
    assert.deepStrictEqual(others, [], "one Verter LSP before the crash");

    const mark = logMark();
    process.kill(crashed.pid, "SIGKILL");
    await waitForReplacementServer(new Set([crashed.pid]), mark);

    const log = readTestLog().slice(mark);
    assert.match(log, /Verter language server lost: the server process exited/);
    assert.match(log, /Automatic language server restart 1\/\d+/);
    assert.doesNotMatch(
      log,
      /Server will restart/,
      "the language client must not restart the server behind the extension's recovery",
    );
    assert.strictEqual(
      await waitForProbeDiagnostic(),
      before,
      "the replacement server must diagnose the editor's unsaved buffer, once",
    );
  });
});
