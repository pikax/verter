import { window, workspace, commands, ExtensionContext, Uri, type LogOutputChannel } from "vscode";
import { existsSync, readFileSync, writeFileSync, mkdirSync } from "fs";
import { join } from "path";
import { homedir } from "os";

const SUPPRESSION_KEY = "verter.mcp.claudeCodeNotificationDismissed";

/**
 * Check if Claude Code is installed and show a notification to set up MCP.
 *
 * Skips if:
 * - `verter.mcp.claudeCodeNotification` is `false`
 * - The notification was already dismissed (persisted in globalState)
 * - Claude Code is not detected
 * - `.mcp.json` in the workspace already has verter configured
 */
export function checkClaudeCodeAndNotify(context: ExtensionContext, log: LogOutputChannel): void {
  const config = workspace.getConfiguration("verter");
  if (!config.get<boolean>("mcp.claudeCodeNotification", true)) return;
  if (context.globalState.get<boolean>(SUPPRESSION_KEY)) return;

  if (!isClaudeCodeInstalled()) return;

  const workspaceRoot = workspace.workspaceFolders?.[0]?.uri.fsPath;
  if (workspaceRoot && isMcpAlreadyConfigured(workspaceRoot)) return;

  log.info("Claude Code detected — showing MCP setup notification");

  window
    .showInformationMessage(
      "Claude Code detected. Set up Verter's MCP server to give Claude access to Vue analysis tools?",
      "Setup Now",
      "Don't Show Again",
    )
    .then((choice) => {
      if (choice === "Setup Now") {
        commands.executeCommand("verter.setupMcpForClaudeCode");
      } else if (choice === "Don't Show Again") {
        context.globalState.update(SUPPRESSION_KEY, true);
      }
    });
}

/**
 * Write/update `.mcp.json` in the workspace root with the Verter MCP server config.
 */
export function setupMcpForClaudeCode(context: ExtensionContext, log: LogOutputChannel): void {
  const workspaceRoot = workspace.workspaceFolders?.[0]?.uri.fsPath;
  if (!workspaceRoot) {
    window.showWarningMessage("No workspace folder open. Open a project first.");
    return;
  }

  const mcpJsonPath = join(workspaceRoot, ".mcp.json");

  let mcpConfig: Record<string, unknown> = {};
  if (existsSync(mcpJsonPath)) {
    try {
      mcpConfig = JSON.parse(readFileSync(mcpJsonPath, "utf-8"));
    } catch {
      log.warn(`Failed to parse existing ${mcpJsonPath}, creating new file`);
    }
  }

  // Ensure mcpServers key exists
  if (!mcpConfig.mcpServers || typeof mcpConfig.mcpServers !== "object") {
    mcpConfig.mcpServers = {};
  }

  // Add/update the verter entry with a placeholder URL.
  // The actual port is dynamic (OS-assigned) and will be updated
  // by updateMcpPort() when the MCP server sends $/verter/mcpReady.
  (mcpConfig.mcpServers as Record<string, unknown>).verter = {
    url: `http://localhost:0/mcp`,
  };

  writeFileSync(mcpJsonPath, JSON.stringify(mcpConfig, null, 2) + "\n", "utf-8");

  log.info(`Wrote MCP config to ${mcpJsonPath}`);
  window.showInformationMessage(
    "Verter MCP configured in .mcp.json. The port will be updated automatically when the server starts. Restart Claude Code to activate.",
  );
}

/** Detect if Claude Code is installed by checking for ~/.claude/ directory. */
function isClaudeCodeInstalled(): boolean {
  if (process.env.CLAUDE_CODE === "1") return true;
  try {
    const claudeDir = join(homedir(), ".claude");
    return existsSync(claudeDir);
  } catch {
    return false;
  }
}

/**
 * Update the verter entry in `.mcp.json` with the actual MCP port.
 * Called when the LSP sends `$/verter/mcpReady` with the dynamic port.
 * Only writes if `.mcp.json` already has a verter entry (avoids creating
 * the file for users who haven't opted in).
 */
export function updateMcpPort(workspaceRoot: string, port: number, log: LogOutputChannel): void {
  const mcpJsonPath = join(workspaceRoot, ".mcp.json");
  let mcpConfig: Record<string, unknown> = {};
  if (existsSync(mcpJsonPath)) {
    try {
      mcpConfig = JSON.parse(readFileSync(mcpJsonPath, "utf-8"));
    } catch {
      return; // Malformed file — don't overwrite
    }
  } else {
    return; // No .mcp.json — user hasn't opted in
  }

  if (!mcpConfig.mcpServers || typeof mcpConfig.mcpServers !== "object") return;
  const servers = mcpConfig.mcpServers as Record<string, unknown>;
  if (!servers.verter) return; // No verter entry — don't create one

  servers.verter = { url: `http://localhost:${port}/mcp` };
  writeFileSync(mcpJsonPath, JSON.stringify(mcpConfig, null, 2) + "\n", "utf-8");
  log.info(`Updated .mcp.json with MCP port ${port}`);
}

/** Check if .mcp.json already has a verter entry. */
function isMcpAlreadyConfigured(workspaceRoot: string): boolean {
  try {
    const mcpJsonPath = join(workspaceRoot, ".mcp.json");
    if (!existsSync(mcpJsonPath)) return false;
    const config = JSON.parse(readFileSync(mcpJsonPath, "utf-8"));
    return !!config?.mcpServers?.verter;
  } catch {
    return false;
  }
}
