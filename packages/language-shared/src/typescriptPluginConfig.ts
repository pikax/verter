/**
 * Plugin configuration set by an editor integration that already registers
 * framework source extensions with TypeScript. In that surface, `.vue` and
 * `.svelte` files are configured-project roots and must not be returned again
 * from the plugin's `getExternalFiles` hook; only their distinct generated
 * companion roots are plugin-owned.
 */
export const EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY = "editorOwnsCarrierMembership";

/**
 * Whether this editor TypeScript project is the selected semantic owner for
 * framework-source features (completion, hover, navigation, rename, fixes).
 *
 * This is deliberately independent from carrier membership. A managed/shared
 * tsgo route still needs the editor plugin to resolve `.vue`/`.svelte` imports
 * from ordinary TypeScript files, but must not contribute a second source
 * completion provider that VS Code merges with the selected tsgo route.
 */
export const EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY = "editorOwnsCarrierSourceFeatures";

/**
 * Test-only attribution rail: TypeScript must own every completion item while
 * Verter remains responsible for carrier publication and source mapping.
 * Integrations must never set this outside an E2E process.
 */
export const E2E_PROVIDER_ONLY_COMPLETIONS_CONFIG_KEY = "e2eProviderOnlyCompletions";

/**
 * Monotonic editor-side token advanced after the LSP has published and synced a
 * new carrier-store generation. The store directory is intentionally stable for
 * a workspace, so its path alone cannot tell tsserver that ready companion roots
 * or their content changed. A token transition requests one targeted configured-
 * project reload; repeated delivery of the same token is a no-op.
 */
export const CARRIER_STORE_REFRESH_TOKEN_CONFIG_KEY = "carrierStoreRefreshToken";

/**
 * Snapshot of the internal LSP tsserver's active carrier working set.
 * Publication and activation are deliberately separate: the manifest may hold
 * every compiled carrier, while only these authored source identities are
 * eligible to become configured-project roots. The plugin serves generated
 * snapshots under those identities, matching official framework plugins.
 */
export const ACTIVE_CARRIER_SOURCES_CONFIG_KEY = "activeCarrierSources";

export function activeCarrierSources(
  config: { readonly [ACTIVE_CARRIER_SOURCES_CONFIG_KEY]?: unknown } | undefined,
): readonly string[] {
  const value = config?.[ACTIVE_CARRIER_SOURCES_CONFIG_KEY];
  if (!Array.isArray(value)) return [];
  return value.filter((candidate): candidate is string => typeof candidate === "string");
}

export function editorOwnsCarrierMembership(
  config: { readonly [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]?: unknown } | undefined,
): boolean {
  return config?.[EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY] === true;
}

export function editorOwnsCarrierSourceFeatures(
  config:
    | {
        readonly [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]?: unknown;
      }
    | undefined,
): boolean {
  return config?.[EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY] === true;
}

export function e2eProviderOnlyCompletions(
  config: { readonly [E2E_PROVIDER_ONLY_COMPLETIONS_CONFIG_KEY]?: unknown } | undefined,
): boolean {
  return config?.[E2E_PROVIDER_ONLY_COMPLETIONS_CONFIG_KEY] === true;
}
