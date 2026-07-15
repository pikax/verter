/**
 * Plugin configuration set by an editor integration that already registers
 * framework source extensions with TypeScript. In that surface, `.vue` and
 * `.svelte` files are configured-project roots and must not be returned again
 * from the plugin's `getExternalFiles` hook; only their distinct generated
 * companion roots are plugin-owned.
 */
export const EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY = "editorOwnsCarrierMembership";

/**
 * Monotonic editor-side token advanced after the LSP has published and synced a
 * new carrier-store generation. The store directory is intentionally stable for
 * a workspace, so its path alone cannot tell tsserver that ready companion roots
 * or their content changed. A token transition requests one targeted configured-
 * project reload; repeated delivery of the same token is a no-op.
 */
export const CARRIER_STORE_REFRESH_TOKEN_CONFIG_KEY = "carrierStoreRefreshToken";

export function editorOwnsCarrierMembership(
  config: { readonly [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]?: unknown } | undefined,
): boolean {
  return config?.[EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY] === true;
}
