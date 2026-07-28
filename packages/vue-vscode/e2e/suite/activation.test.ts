import { expect } from "chai";
import * as fs from "fs";
import * as http from "http";
import * as path from "path";
import { pollBudget } from "../lib/timeouts";
import * as vscode from "vscode";
import {
  ensureFixtureWarm,
  readTestLog,
  assertLogContains,
  assertLogNotContains,
  isLspReady,
  FIXTURE_NAME,
} from "../helpers";

suite(`Activation & LSP Health [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    await ensureFixtureWarm();
  });

  /**
   * Poll the extension log for a line the standalone MCP child produces
   * asynchronously. Returns quietly on timeout — the caller's assertion
   * produces the real failure message against the final log.
   */
  async function waitForLogToContain(needle: string): Promise<void> {
    const deadline = Date.now() + pollBudget("activationMcpReady");
    while (Date.now() < deadline) {
      if (readTestLog().includes(needle)) return;
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
  }

  test("extension activates successfully", function () {
    const ext = vscode.extensions.getExtension("verter.verter-vscode");
    expect(ext, "Extension should be found").to.exist;
    expect(ext!.isActive, "Extension should be active").to.be.true;
  });

  test("Verter output channel was created", function () {
    assertLogContains("Verter extension activating", "Log should contain activation message");
  });

  test("LSP binary was found and started", function () {
    assertLogContains("LSP binary:", "Log should indicate which LSP binary was used");
  });

  test("server sends $/verter/ready notification", function () {
    expect(isLspReady(), "LSP should reach ready state").to.be.true;
    assertLogContains("Verter ready", "Extension should log the ready notification");
  });

  test("server sends heartbeat", async function () {
    expect(isLspReady(), "LSP should reach ready state").to.be.true;
    this.timeout(15_000);
    // Heartbeat is sent every 5s — wait long enough for at least one
    const start = Date.now();
    while (Date.now() - start < pollBudget("activationHeartbeat")) {
      const log = readTestLog();
      // Look for the actual heartbeat notification, not error messages about missing heartbeats
      if (log.includes("$/verter/heartbeat")) {
        return; // Found heartbeat
      }
      await new Promise((r) => setTimeout(r, 1000));
    }
    // If LSP is ready, heartbeat should exist
    const log = readTestLog();
    expect(
      log.includes("$/verter/heartbeat"),
      "Should receive $/verter/heartbeat notifications from LSP",
    ).to.be.true;
  });

  test("standalone MCP server reports a valid bound port", async function () {
    expect(isLspReady(), "LSP should reach ready state").to.be.true;

    // The standalone verter-mcp child starts in parallel with the LSP, so its
    // readiness may trail the LSP ready line — poll, then assert.
    await waitForLogToContain("MCP HTTP server ready on port");
    const log = readTestLog();

    // The extension logs this only after parsing the standalone child's stable
    // readiness record; human stderr logs are not accepted as port identity.
    assertLogContains(
      "MCP HTTP server ready on port",
      "Extension should log the standalone MCP readiness record",
    );

    // Verify port is a valid number
    const portMatch = log.match(/MCP HTTP server ready on port (\d+)/);
    expect(portMatch, "Log should contain a port number").to.exist;
    const port = parseInt(portMatch![1], 10);
    expect(port, "Port should be > 0").to.be.greaterThan(0);
    expect(port, "Port should be < 65536").to.be.lessThan(65536);

    // The advertised port must be a LIVE listener, not a log claim: connect
    // to the endpoint and require an HTTP response. A fabricated or stale
    // port refuses the connection and fails this.
    const statusCode = await new Promise<number>((resolve, reject) => {
      const request = http.get({ host: "127.0.0.1", port, path: "/mcp" }, (response) => {
        response.resume();
        resolve(response.statusCode ?? 0);
      });
      request.on("error", reject);
      request.setTimeout(5_000, () => request.destroy(new Error("MCP endpoint probe timed out")));
    });
    expect(statusCode, "the advertised MCP endpoint must answer HTTP").to.be.greaterThan(0);
  });

  test("MCP server registered with VS Code", async function () {
    expect(isLspReady(), "LSP should reach ready state").to.be.true;
    await waitForLogToContain("Registered MCP server with VS Code");
    assertLogContains(
      "Registered MCP server with VS Code",
      "Extension should log successful MCP provider registration",
    );

    // Registration must have REACHED VS Code's MCP service, not merely have
    // been logged: on a real registration VS Code itself pulls the server
    // definitions from the provider, and only that pull produces this line.
    // A no-op'd `registerMcpServerDefinitionProvider` never does.
    await waitForLogToContain("MCP server definitions pulled by VS Code");
    const log = readTestLog();
    const pullMatch = log.match(/MCP server definitions pulled by VS Code \(port (\d+)\)/);
    expect(pullMatch, "VS Code should have pulled the registered MCP server definitions").to.exist;

    // The definition VS Code pulled advertises the SAME port the readiness
    // record announced — the registration serves the live endpoint.
    const readyMatch = log.match(/MCP HTTP server ready on port (\d+)/);
    expect(readyMatch, "readiness line must exist alongside the pull").to.exist;
    expect(pullMatch![1], "pulled definition port must match the bound port").to.equal(
      readyMatch![1],
    );

    assertLogNotContains(
      "Failed to register MCP server",
      "MCP registration should not have failed",
    );
  });

  test("setup command (WARM path) writes the LIVE MCP endpoint to .mcp.json, never a placeholder", async function () {
    // Scope, stated honestly: this suite's suiteSetup warms the LSP and the
    // MCP child before any test runs, so this leg proves the WARM path only —
    // the command reads the cached live endpoint and writes it. The COLD path
    // ("invoking Setup when the server is not yet running still results in a
    // live endpoint being written") cannot be reached from this shared warm
    // VS Code instance; it is covered at unit level by the `runMcpSetupCommand`
    // suite in src/mcpServer.spec.ts (cold start kick + failed-MCP retry).
    expect(isLspReady(), "LSP should reach ready state").to.be.true;
    await waitForLogToContain("MCP HTTP server ready on port");
    const readyMatch = readTestLog().match(/MCP HTTP server ready on port (\d+)/);
    expect(readyMatch, "readiness line must exist before invoking setup").to.exist;
    const port = readyMatch![1];

    const wsRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    expect(wsRoot, "fixture must have a workspace folder").to.exist;
    const mcpJsonPath = path.join(wsRoot!, ".mcp.json");
    // Snapshot whatever exists, then DELETE before invoking: the readiness
    // path auto-writes .mcp.json on every onReady (extension.ts), and a prior
    // interrupted run may have left one behind — either would false-green the
    // write assertion below. After the delete, only the command itself can
    // produce the file.
    const priorBytes = fs.existsSync(mcpJsonPath) ? fs.readFileSync(mcpJsonPath) : undefined;
    fs.rmSync(mcpJsonPath, { force: true });
    try {
      await vscode.commands.executeCommand("verter.setupMcpForClaudeCode");
      expect(
        fs.existsSync(mcpJsonPath),
        "the setup COMMAND itself should have written .mcp.json (it was deleted before invoking)",
      ).to.be.true;
      const written = JSON.parse(fs.readFileSync(mcpJsonPath, "utf-8")) as {
        mcpServers?: Record<string, { url?: string }>;
      };
      const url = written.mcpServers?.verter?.url;
      // The REAL bound port, on the bind address — never the dead
      // `http://127.0.0.1:0/mcp` placeholder, never `localhost`.
      expect(url, "setup must write the live endpoint").to.equal(`http://127.0.0.1:${port}/mcp`);
      expect(url).to.not.contain(":0/");
      expect(url).to.not.contain("localhost");
    } finally {
      // Leave the tracked fixture directory exactly as found: restore the
      // prior bytes when a file existed, remove the file when none did.
      if (priorBytes === undefined) {
        fs.rmSync(mcpJsonPath, { force: true });
      } else {
        fs.writeFileSync(mcpJsonPath, priorBytes);
      }
    }
  });

  test("no panics or crashes in log", function () {
    assertLogNotContains("panicked at", "Should not have Rust panics");
    assertLogNotContains("thread 'main' panicked", "Should not have thread panics");
  });

  test("tsserver does not inherit debugger env vars", function () {
    // Under F5 sessions, VS Code sets NODE_OPTIONS/VSCODE_INSPECTOR_OPTIONS
    // which cause tsserver to open a debug port. The env sanitization fix
    // strips these vars. This test validates no debugger noise in the log.
    assertLogNotContains(
      "Debugger listening",
      "tsserver should not open a debug port (env sanitization)",
    );
    assertLogNotContains(
      "Debugger attached",
      "tsserver should not have debugger attached (env sanitization)",
    );
  });

  test("log file is non-empty", function () {
    const log = readTestLog();
    expect(log.length, "Log file should have content").to.be.greaterThan(0);
  });
});
