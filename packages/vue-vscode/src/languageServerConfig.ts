export interface ConfigurationChangeLike {
  affectsConfiguration(section: string): boolean;
}

const RESTART_REQUIRED_SETTINGS = [
  "verter.server.logLevel",
  "verter.typeProvider",
  "verter.typescript.tsdk",
  "verter.analysis.enabled",
  "verter.mcp.enabled",
  "verter.mcp.port",
  "verter.inlayHints.enabled",
  "verter.viteConfig.enabled",
  "verter.viteConfig.trustedFiles",
  "verter.experimental.conditionalRootNarrowing",
  "verter.experimental.strictSlots",
  "verter.hover.nativeSemantics",
  "verter.hover.provenance",
] as const;

export function shouldRestartLanguageServerForConfigurationChange(
  event: ConfigurationChangeLike,
): boolean {
  return RESTART_REQUIRED_SETTINGS.some((setting) => event.affectsConfiguration(setting));
}

/**
 * The `--tsdk` launch arg for the LSP, derived ONLY from the user's
 * `verter.typescript.tsdk` setting (`""` when unset).
 *
 * There is deliberately no bundled default: the LSP's tsserver discovery
 * already searches project-local installs (walking every ancestor of the
 * owning project) before the configured tsdk, then a global install, and
 * fails closed with an actionable error when nothing resolves. Injecting the
 * extension's own TypeScript into that cascade would silently serve a
 * TypeScript the project does not use.
 */
export function lspTsdkLaunchArg(userTsdk: string): string | undefined {
  return userTsdk ? `--tsdk=${userTsdk}` : undefined;
}

/** Everything the LSP launch argv is derived from. */
export interface LspLaunchArgsInput {
  /** The client-process-lifetime flag (`--client-pid=…`). */
  readonly clientProcessLifetimeArg: string;
  /** `verter.typeProvider` (or its E2E override). */
  readonly typeProvider: string;
  /** `verter.typescript.tsdk` — `""` when the user has not configured one. */
  readonly userTsdk: string;
  /** Directory handed to `--plugin-path`. */
  readonly pluginPath: string;
  /** MCP flags, omitted entirely when MCP is disabled. */
  readonly mcp?: { readonly port: number; readonly lintPreset: string };
  /** Attested editor-serving facts, already `--`-prefixed. */
  readonly sharedLspArgs?: readonly string[];
  /** The positional workspace root the LSP parses last. */
  readonly rootPath?: string;
}

/**
 * Assemble the LSP launch argv — the exact array handed to `ServerOptions`.
 *
 * This is the public launch boundary: if a bundled-TypeScript `--tsdk` default
 * is ever reintroduced (from `extensionPath/node_modules/typescript/lib` or
 * anywhere else), it shows up here, in the argv the server is actually spawned
 * with. `--tsdk` is emitted ONLY from the user's setting.
 */
export function buildLspLaunchArgs(input: LspLaunchArgsInput): string[] {
  const args: string[] = [];
  args.push(input.clientProcessLifetimeArg);
  args.push(`--type-provider=${input.typeProvider}`);
  const tsdkArg = lspTsdkLaunchArg(input.userTsdk);
  if (tsdkArg) args.push(tsdkArg);
  args.push(`--plugin-path=${input.pluginPath}`);
  if (input.mcp) {
    args.push(`--mcp-port=${input.mcp.port}`);
    args.push(`--mcp-lint-preset=${input.mcp.lintPreset}`);
  }
  // Attested facts are `--`-prefixed, so they precede the positional root.
  args.push(...(input.sharedLspArgs ?? []));
  if (input.rootPath) args.push(input.rootPath);
  return args;
}
