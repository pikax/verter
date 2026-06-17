import {
  commands,
  CompletionItemKind,
  languages,
  workspace,
  type CompletionList,
  type LogOutputChannel,
  type TextDocument,
  type Uri,
} from "vscode";
import { appendFileSync } from "fs";
import { isFrameworkCarrierLanguageId } from "./frameworkWiring";

export interface StartupProbeConfig {
  relativePath: string;
  completionAnchor: string;
  completionLabel: string;
  completionKinds?: string[];
  pollIntervalMs?: number;
  timeoutMs?: number;
}

export interface ParsedStartupTiming {
  activationStartMs?: number;
  typeProviderStartedMs?: number;
  lspReadyMs?: number;
  firstTypedCompletionMs?: number;
  firstDiagnosticMs?: number;
  providerKind?: "tsgo" | "tsserver" | "verter-only";
}

const DEFAULT_TIMEOUT_MS = 45_000;
const DEFAULT_POLL_INTERVAL_MS = 50;

export function readStartupProbeConfig(): StartupProbeConfig | undefined {
  const raw = process.env.VERTER_E2E_STARTUP_PROBE;
  if (!raw) {
    return undefined;
  }

  try {
    const parsed = JSON.parse(raw) as Partial<StartupProbeConfig>;
    if (
      !parsed ||
      typeof parsed.relativePath !== "string" ||
      typeof parsed.completionAnchor !== "string" ||
      typeof parsed.completionLabel !== "string"
    ) {
      return undefined;
    }

    return {
      relativePath: normalizePath(parsed.relativePath),
      completionAnchor: parsed.completionAnchor,
      completionLabel: parsed.completionLabel,
      completionKinds: Array.isArray(parsed.completionKinds)
        ? parsed.completionKinds.filter(
            (kind): kind is string => typeof kind === "string" && kind.length > 0,
          )
        : undefined,
      pollIntervalMs: parsed.pollIntervalMs,
      timeoutMs: parsed.timeoutMs,
    };
  } catch {
    return undefined;
  }
}

export function writeTimingMarker(name: string, ...parts: Array<string | number>): void {
  const testLogFile = process.env.VERTER_E2E_LOG_FILE;
  if (!process.env.VERTER_E2E_TEST || !testLogFile) {
    return;
  }

  const suffix = parts.length > 0 ? ` ${parts.join(" ")}` : "";
  try {
    appendFileSync(testLogFile, `[TIMING] ${name}${suffix}\n`);
  } catch {
    // Best-effort test instrumentation only.
  }
}

export class StartupProbe {
  private targetUri?: string;
  private completionTask?: Promise<void>;
  private firstDiagnosticLogged = false;
  private disposed = false;

  constructor(
    private readonly config: StartupProbeConfig,
    private readonly log: LogOutputChannel,
  ) {}

  maybeTrackDocument(document: TextDocument): void {
    if (!this.isTargetDocument(document)) {
      return;
    }

    this.targetUri = document.uri.toString();
    if (!this.completionTask) {
      this.completionTask = this.trackTypedCompletion(document);
    }
  }

  maybeTrackDiagnostics(uri: Uri): void {
    if (this.firstDiagnosticLogged || !this.targetUri || uri.toString() !== this.targetUri) {
      return;
    }

    if (languages.getDiagnostics(uri).length === 0) {
      return;
    }

    this.firstDiagnosticLogged = true;
    writeTimingMarker("first_diagnostic", Date.now());
  }

  markTypeProviderStarted(kind: "tsgo" | "tsserver"): void {
    writeTimingMarker("type_provider_started", Date.now(), kind);
  }

  markReady(): void {
    writeTimingMarker("ready", Date.now());
  }

  dispose(): void {
    this.disposed = true;
  }

  private isTargetDocument(document: TextDocument): boolean {
    if (!isFrameworkCarrierLanguageId(document.languageId)) {
      return false;
    }

    const workspaceFolder = workspace.getWorkspaceFolder(document.uri);
    if (!workspaceFolder) {
      return false;
    }

    const relativePath = normalizePath(workspace.asRelativePath(document.uri, false));

    return relativePath === this.config.relativePath;
  }

  private findProbeOffset(source: string): number | undefined {
    const anchorOffset = source.indexOf(this.config.completionAnchor);
    if (anchorOffset === -1) {
      return undefined;
    }

    const labelOffset = this.config.completionAnchor.indexOf(this.config.completionLabel);
    if (labelOffset === -1) {
      return anchorOffset;
    }

    return anchorOffset + labelOffset;
  }

  private async trackTypedCompletion(document: TextDocument): Promise<void> {
    const probeOffset = this.findProbeOffset(document.getText());
    if (probeOffset === undefined) {
      this.log.warn(
        `Startup probe anchor "${this.config.completionAnchor}" was not found in ${document.uri.fsPath}`,
      );
      return;
    }

    const position = document.positionAt(probeOffset);
    const timeoutMs = this.config.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    const pollIntervalMs = this.config.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS;
    const start = Date.now();

    while (!this.disposed && Date.now() - start < timeoutMs) {
      const completions = await commands.executeCommand<CompletionList | undefined>(
        "vscode.executeCompletionItemProvider",
        document.uri,
        position,
      );

      const match = completions?.items.find((item) => item.label === this.config.completionLabel);
      if (
        match &&
        match.kind !== undefined &&
        matchesExpectedCompletionKind(match.kind, this.config.completionKinds)
      ) {
        writeTimingMarker(
          "first_typed_completion",
          Date.now(),
          this.config.completionLabel,
          CompletionItemKind[match.kind] ?? String(match.kind),
        );
        return;
      }

      await sleep(pollIntervalMs);
    }

    this.log.warn(
      `Startup probe timed out waiting for typed completion "${this.config.completionLabel}"`,
    );
  }
}

function normalizePath(value: string): string {
  return value.replace(/\\/g, "/");
}

function matchesExpectedCompletionKind(
  actualKind: CompletionItemKind,
  expectedKinds?: readonly string[],
): boolean {
  if (expectedKinds && expectedKinds.length > 0) {
    const actualKindName = CompletionItemKind[actualKind];
    return actualKindName !== undefined && expectedKinds.includes(actualKindName);
  }
  return actualKind !== CompletionItemKind.Text;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
