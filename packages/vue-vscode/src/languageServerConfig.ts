export interface ConfigurationChangeLike {
  affectsConfiguration(section: string): boolean;
}

const RESTART_REQUIRED_SETTINGS = [
  "verter.server.logLevel",
  "verter.typeProvider",
  "verter.typescript.tsdk",
  "verter.mcp.enabled",
  "verter.mcp.port",
  "verter.inlayHints.enabled",
  "verter.viteConfig.enabled",
  "verter.viteConfig.trustedFiles",
  "verter.experimental.conditionalRootNarrowing",
  "verter.experimental.strictSlots",
  "verter.experimental.deepComponentMetaExpansion",
] as const;

export function shouldRestartLanguageServerForConfigurationChange(
  event: ConfigurationChangeLike,
): boolean {
  return RESTART_REQUIRED_SETTINGS.some((setting) => event.affectsConfiguration(setting));
}
