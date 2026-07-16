import { expect } from "chai";
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

  test("extension activates successfully", function () {
    const ext = vscode.extensions.getExtension("pikax.verter-vscode");
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
    while (Date.now() - start < 12_000) {
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

  test("standalone MCP server reports a valid bound port", function () {
    expect(isLspReady(), "LSP should reach ready state").to.be.true;

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
  });

  test("MCP server registered with VS Code", function () {
    expect(isLspReady(), "LSP should reach ready state").to.be.true;
    assertLogContains(
      "Registered MCP server with VS Code",
      "Extension should log successful MCP provider registration",
    );
    assertLogNotContains(
      "Failed to register MCP server",
      "MCP registration should not have failed",
    );
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
