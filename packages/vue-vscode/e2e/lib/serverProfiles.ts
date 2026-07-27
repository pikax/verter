/**
 * The server configuration a suite is executed against.
 *
 * Most of the E2E tree is configuration-independent and runs on `default` — the
 * settings an editor that wires nothing gets, which is what an acceptance
 * fixture should be measuring.
 *
 * `verter-native-semantics` is the DOCUMENTED opt-in lane. Verter's native
 * hover contribution and its semantic-enrichment snapshot are deliberately off
 * by default (`packages/vue-vscode/package.json` ships both as `false`, and
 * `crates/verter_lsp/src/config.rs::parse_hover_init_options` defaults the
 * server side to `false` too), so a suite asserting those affordances has to
 * request them. Asserting them on `default` is asserting a capability the
 * product does not claim to offer there — which is exactly how seven hover
 * assertions came to fail on a correct implementation.
 */
export type E2eServerProfile = "default" | "verter-native-semantics";

/** The profile a suite runs under unless it asks for another. */
export const DEFAULT_SERVER_PROFILE: E2eServerProfile = "default";

/**
 * VS Code settings for each profile — the single authority shared by the
 * launcher (which writes the route's baseline into the isolated user-data dir)
 * and the in-host activator (which flips a suite onto its declared profile), so
 * a suite's declared profile and the server it actually talks to cannot drift.
 *
 * `verter.hover.nativeSemantics` gates the native hover lane; without it
 * `nav_features.rs` never consults `hover_at_position`, so the source-owned
 * `@click` / `.prevent` / slot-outlet hovers are unreachable and the provider's
 * generated-TSX answer is all there is. `verter.analysis.enabled` gates the
 * semantic-enrichment snapshot carrying the TEMPLATE half of the analysis;
 * without it the markup-side native features have nothing to resolve against.
 * The native hover lane needs BOTH.
 *
 * Every profile lists every key, so switching profiles is a total assignment
 * and no key can be left behind at a previous profile's value.
 */
export const E2E_SERVER_PROFILES: Readonly<
  Record<E2eServerProfile, Readonly<Record<string, boolean>>>
> = {
  default: {
    "verter.hover.nativeSemantics": false,
    "verter.analysis.enabled": false,
  },
  "verter-native-semantics": {
    "verter.hover.nativeSemantics": true,
    "verter.analysis.enabled": true,
  },
};

/**
 * The environment variable carrying a route's launch profile into the extension
 * host, so a suite can tell what the server was STARTED with rather than
 * guessing from the current settings (which a previous suite may have flipped).
 */
export const E2E_SERVER_PROFILE_ENV = "SERVER_PROFILE";

/**
 * The environment variable carrying a route's BASELINE profile — the one a suite
 * that declares nothing runs under. Suite selection needs both: the baseline to
 * resolve each suite's profile, and {@link E2E_SERVER_PROFILE_ENV} to know which
 * of those this launch is serving.
 */
export const E2E_BASE_SERVER_PROFILE_ENV = "BASE_SERVER_PROFILE";

/** Every settings key any profile controls. */
export function serverProfileKeys(): readonly string[] {
  const keys = new Set<string>();
  for (const settings of Object.values(E2E_SERVER_PROFILES)) {
    for (const key of Object.keys(settings)) keys.add(key);
  }
  return [...keys].sort();
}

/** The settings a profile asks for, as a total assignment over every key. */
export function serverProfileSettings(profile: E2eServerProfile): Record<string, boolean> {
  const declared = E2E_SERVER_PROFILES[profile];
  const settings: Record<string, boolean> = {};
  for (const key of serverProfileKeys()) {
    const value = declared[key];
    if (value === undefined) {
      throw new Error(
        `server profile "${profile}" does not declare "${key}"; every profile must be total ` +
          "so a profile switch cannot leave a key at the previous profile's value",
      );
    }
    settings[key] = value;
  }
  return settings;
}

/**
 * A SHORT tag for a profile, for use inside on-disk names.
 *
 * The VS Code user-data directory's path becomes a Unix domain socket path, and
 * those are capped at ~103 bytes; the full profile name pushed
 * `verter-e2e-profile-<pid>-<n>-single-project-tsgo-verter-native-semantics/user-data/1.13-main.sock`
 * past it and VS Code refused to start with `listen EINVAL`.
 */
export const E2E_SERVER_PROFILE_SLUGS: Readonly<Record<E2eServerProfile, string>> = {
  default: "def",
  "verter-native-semantics": "native",
};

/** Whether a string names a known profile. */
export function isE2eServerProfile(value: string | undefined): value is E2eServerProfile {
  return value !== undefined && Object.prototype.hasOwnProperty.call(E2E_SERVER_PROFILES, value);
}

/**
 * Suites that assert an OPT-IN surface, and the profile they need.
 *
 * A suite is the unit here because a VS Code launch configures one server, and
 * Verter's native lane is an INITIALIZATION option: the running server cannot be
 * reconfigured — `packages/vue-vscode/src/extension.ts` builds
 * `clientOptions.initializationOptions` once at activation and re-sends that same
 * frozen object on restart, so flipping the setting mid-run restarts the server
 * with the OLD options (measured: `native hover semantics: disabled (default)` on
 * every restart after a confirmed `true` in the extension host's configuration).
 * Each profile therefore gets its own launch, exactly as the editor-neutral
 * contract gives each profile its own server.
 *
 * Keys are the suite path under `e2e/suite/` without the `.ts`, matching the
 * spelling in `fixtureSuiteMap.ts`.
 */
export const SUITE_SERVER_PROFILES: Readonly<Record<string, E2eServerProfile>> = {
  // Source-owned `@click` / `.prevent` / slot-outlet hovers, which the IDE codegen
  // renames or deletes on the way to TSX — no TypeScript provider can describe them.
  "hover.test": "verter-native-semantics",
  // `generic=` / `attrs=` attribute-NAME hovers are SFC-syntax documentation with
  // no generated token to hover.
  "generic-attrs.test": "verter-native-semantics",
};

/**
 * The profile a suite runs under on a route whose baseline is `baseProfile`.
 *
 * A suite that declares nothing inherits the route's baseline, so the parity
 * fixtures — which launch opted in wholesale — keep running as one profile
 * rather than being split by suites that never load there.
 */
export function serverProfileForSuite(
  suiteRelPosix: string,
  baseProfile: E2eServerProfile,
): E2eServerProfile {
  const normalized = suiteRelPosix.replace(/\\/g, "/").replace(/\.(?:js|ts)$/, "");
  for (const [suite, profile] of Object.entries(SUITE_SERVER_PROFILES)) {
    if (normalized.includes(suite)) return profile;
  }
  return baseProfile;
}

/**
 * The distinct profiles a set of suites needs, in a stable order with the
 * baseline first.
 *
 * The caller passes the fixture's suite GLOBS (`fixtureSuiteMap.ts`), not the
 * authored-source inventory, so this DERIVES profiles from what a fixture is
 * configured to load rather than DISCOVERING them from the tree. That is enough
 * for a suite declaring a new profile to get its own launch instead of silently
 * running on the wrong one — a suite only runs if a glob admits it — but it is a
 * derivation from configuration, not a discovery from disk.
 */
export function serverProfilesForSuites(
  suiteRelPaths: readonly string[],
  baseProfile: E2eServerProfile,
): readonly E2eServerProfile[] {
  const needed = new Set<E2eServerProfile>();
  for (const suite of suiteRelPaths) {
    needed.add(serverProfileForSuite(suite, baseProfile));
  }
  if (needed.size === 0) return [baseProfile];
  return [
    ...(needed.has(baseProfile) ? [baseProfile] : []),
    ...[...needed].filter((profile) => profile !== baseProfile).sort(),
  ];
}
