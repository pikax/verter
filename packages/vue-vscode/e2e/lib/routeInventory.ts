export const TYPE_PROVIDER_ROUTES = ["tsserver", "tsgo", "shared-tsgo"] as const;

/**
 * Routes outside the standard fixture matrix. `editor-tsserver` is the explicit
 * editor-owned tier: it is never selected automatically, so it is exercised only
 * by the acceptance fixture that owns it.
 */
export const NON_MATRIX_TYPE_PROVIDER_ROUTES = ["editor-tsserver"] as const;

export type E2eTypeProviderRoute =
  | (typeof TYPE_PROVIDER_ROUTES)[number]
  | (typeof NON_MATRIX_TYPE_PROVIDER_ROUTES)[number];

const SELECTABLE_TYPE_PROVIDER_ROUTES: readonly string[] = [
  ...TYPE_PROVIDER_ROUTES,
  ...NON_MATRIX_TYPE_PROVIDER_ROUTES,
];

export const STANDARD_E2E_FIXTURES = [
  "single-project",
  "monorepo",
  "tsconfig-extends",
  "tsconfig-references",
  "path-aliases",
  "composite-paths",
  "no-config",
  "single-file",
  "barrel-exports",
  "vue-contract",
  "svelte-contract",
  "vue-parity",
  "svelte-parity",
  "mixed-parity",
  "multi-root-parity",
  "ecosystem-parity",
] as const;

/** Shared editor attach requires a configured project binding. */
export const SHARED_TSGO_INAPPLICABLE_FIXTURES = ["no-config", "single-file"] as const;

export interface E2eRoute {
  readonly fixture: string;
  readonly typeProvider: E2eTypeProviderRoute;
}

export const EDITOR_ACCEPTANCE_ROUTES: readonly E2eRoute[] = [
  { fixture: "editor-owned-project", typeProvider: "editor-tsserver" },
  { fixture: "editor-owned-project", typeProvider: "shared-tsgo" },
] as const;

/** The one canonical route inventory consumed by both the local runner and CI. */
export function buildE2eRouteInventory(): E2eRoute[] {
  return [
    ...STANDARD_E2E_FIXTURES.flatMap((fixture) =>
      TYPE_PROVIDER_ROUTES.filter(
        (typeProvider) =>
          typeProvider !== "shared-tsgo" ||
          !SHARED_TSGO_INAPPLICABLE_FIXTURES.includes(
            fixture as (typeof SHARED_TSGO_INAPPLICABLE_FIXTURES)[number],
          ),
      ).map((typeProvider) => ({ fixture, typeProvider })),
    ),
    ...EDITOR_ACCEPTANCE_ROUTES,
  ];
}

export function e2eRouteLabel(route: E2eRoute): string {
  return `${route.fixture}@${route.typeProvider}`;
}

export function parseE2eRouteLabel(label: string): E2eRoute {
  const split = label.lastIndexOf("@");
  if (split <= 0 || split === label.length - 1) {
    throw new Error(`VS Code E2E route must be <fixture>@<provider>, got ${JSON.stringify(label)}`);
  }
  const fixture = label.slice(0, split);
  const typeProvider = label.slice(split + 1);
  if (!SELECTABLE_TYPE_PROVIDER_ROUTES.includes(typeProvider)) {
    throw new Error(
      `Unsupported VS Code E2E provider ${JSON.stringify(typeProvider)}; expected ${SELECTABLE_TYPE_PROVIDER_ROUTES.join(", ")}`,
    );
  }
  const [route] = selectE2eRoutes({ fixture, typeProvider });
  return route;
}

export function selectE2eRoutes(options: {
  readonly fixture?: string;
  readonly typeProvider?: string;
}): E2eRoute[] {
  if (options.typeProvider && !SELECTABLE_TYPE_PROVIDER_ROUTES.includes(options.typeProvider)) {
    throw new Error(`Unsupported VS Code E2E provider ${JSON.stringify(options.typeProvider)}`);
  }
  const selected = buildE2eRouteInventory().filter(
    (route) =>
      (!options.fixture || route.fixture === options.fixture) &&
      (!options.typeProvider || route.typeProvider === options.typeProvider),
  );
  if (selected.length === 0) {
    throw new Error(
      `VS Code E2E route selection matched nothing; fixture=${options.fixture ?? "*"} provider=${options.typeProvider ?? "*"}`,
    );
  }
  return selected;
}

export function buildGitHubActionsMatrix(): {
  readonly include: Array<{
    readonly fixture: string;
    readonly type_provider: E2eTypeProviderRoute;
  }>;
} {
  return {
    include: buildE2eRouteInventory().map(({ fixture, typeProvider }) => ({
      fixture,
      type_provider: typeProvider,
    })),
  };
}
