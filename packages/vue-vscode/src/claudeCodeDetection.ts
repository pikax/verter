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

  const port = workspace.getConfiguration("verter").get<number>("mcp.port", 6772);
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

  // Add/update the verter entry
  (mcpConfig.mcpServers as Record<string, unknown>).verter = {
    url: `http://localhost:${port}/mcp`,
  };

  writeFileSync(mcpJsonPath, JSON.stringify(mcpConfig, null, 2) + "\n", "utf-8");

  log.info(`Wrote MCP config to ${mcpJsonPath}`);
  window.showInformationMessage(
    `Verter MCP configured in .mcp.json (port ${port}). Restart Claude Code to activate.`,
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

/** Check if .mcp.json already has a verter entry. */
function isMcpAlreadyConfigured(workspaceRoot: string): boolean {
  try {
    const mcpJsonPath = join(workspaceRoot, ".mcp.json");
    if (!existsSync(mcpJsonPath)) return false;
    const config = JSON.parse(readFileSync(mcpJsonPath, "utf-8"));
    return !!(config?.mcpServers?.verter);
  } catch {
    return false;
  }
}
