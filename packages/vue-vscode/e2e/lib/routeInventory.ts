export const TYPE_PROVIDER_ROUTES = ["tsserver", "tsgo", "shared-tsgo"] as const;

/**
 * Routes outside the standard fixture matrix. `editor-tsserver` is the explicit
 * editor-owned tier and `extension` the in-extension-host language service:
 * neither is ever selected automatically, so each is exercised only by the
 * acceptance fixture that owns it.
 */
export const NON_MATRIX_TYPE_PROVIDER_ROUTES = ["editor-tsserver", "extension"] as const;

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

/**
 * The extension-hosted provider's acceptance route.
 *
 * Its workspace is materialized OUTSIDE this repository (see
 * `OUT_OF_TREE_FIXTURES` in `runTests.ts`) and that is load-bearing, not
 * incidental: the extension host resolves each project's TypeScript with
 * `createRequire` anchored at the DECLARED project root, and Node's resolution
 * walks up. A fixture living under `packages/vue-vscode/e2e/fixtures/*` therefore
 * finds the REPOSITORY's own `typescript` from any root whatsoever — including a
 * wrongly-declared one — so an in-tree fixture cannot tell a correct
 * project-bound declaration from a folder-derived one. Launched from an OS temp
 * directory, the workspace root has no TypeScript above it at all: declaring the
 * nested package serves, declaring the folder fails closed.
 */
export const EXTENSION_ACCEPTANCE_ROUTES: readonly E2eRoute[] = [
  { fixture: "out-of-tree-monorepo", typeProvider: "extension" },
] as const;

/** A route that stays selectable but is not part of the required matrix. */
export interface DeselectedE2eRoute {
  readonly route: E2eRoute;
  /** The present-tense product fact that keeps the route out of the required set. */
  readonly reason: string;
}

/**
 * Routes the required matrix does not run. Each stays in the inventory and stays
 * selectable by an explicit `--fixture=<fixture>@<provider>` selector; a required
 * run neither schedules it nor depends on it.
 */
export const NON_REQUIRED_E2E_ROUTES: readonly DeselectedE2eRoute[] = [
  {
    route: { fixture: "out-of-tree-monorepo", typeProvider: "extension" },
    reason:
      "carrier publication is suppressed for TypeProviderKind::Tsserver, the kind the extension-hosted service registers under, so no .vue.tsx companion reaches it and its acceptance is skipped; the setting is contained rather than silent — opening a carrier under `extension` warns and names auto/tsserver/tsgo, and the status bar holds a persistent warning while one is open",
  },
] as const;

function isNonRequiredRoute(route: E2eRoute): boolean {
  return NON_REQUIRED_E2E_ROUTES.some(
    ({ route: deselected }) =>
      deselected.fixture === route.fixture && deselected.typeProvider === route.typeProvider,
  );
}

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
    ...EXTENSION_ACCEPTANCE_ROUTES,
  ];
}

/**
 * The required matrix: the canonical inventory minus {@link NON_REQUIRED_E2E_ROUTES}.
 * A deselection naming a route the inventory does not declare is refused, so a
 * stale entry fails loudly instead of silently narrowing the required set.
 */
export function buildRequiredE2eRouteInventory(): E2eRoute[] {
  const inventory = buildE2eRouteInventory();
  for (const { route } of NON_REQUIRED_E2E_ROUTES) {
    const declared = inventory.some(
      (candidate) =>
        candidate.fixture === route.fixture && candidate.typeProvider === route.typeProvider,
    );
    if (!declared) {
      throw new Error(
        `VS Code E2E deselection names a route absent from the inventory: ${e2eRouteLabel(route)}`,
      );
    }
  }
  return inventory.filter((route) => !isNonRequiredRoute(route));
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
  // An unselected run is the required matrix; any explicit selector reaches the
  // whole inventory, so a deselected route stays runnable on demand.
  const searched =
    options.fixture || options.typeProvider
      ? buildE2eRouteInventory()
      : buildRequiredE2eRouteInventory();
  const selected = searched.filter(
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

/**
 * Resolve a launcher's `E2E_FIXTURE` / `E2E_TYPE_PROVIDER` environment into a
 * fixture that is known to exist.
 *
 * The launchers join this value onto `e2e/fixtures/`, so an unchecked one
 * escapes the fixture directory entirely: `E2E_FIXTURE=../..` resolves to
 * `packages/vue-vscode`, which has a `package.json` and a pnpm-managed
 * `node_modules`. That was inert while a launcher only ever SKIPPED an existing
 * `node_modules`; it stopped being inert when deciding about a dependency tree
 * became an action that displaces one. A selector must therefore name a route in
 * the canonical inventory before anything builds a path from it — which is a
 * closed list of literal names, so no traversal spelling can satisfy it.
 *
 * `runTests.ts` was never exposed: it resolves routes through
 * {@link selectE2eRoutes} already. This is that same check, for the launchers
 * that read the environment directly.
 */
export function resolveE2eFixtureSelection(options: {
  readonly rawFixture?: string;
  readonly typeProvider?: string;
}): { readonly fixture: string; readonly typeProvider: string } {
  const raw = options.rawFixture?.trim() || "single-project";
  const split = raw.indexOf("@");
  const fixture = split === -1 ? raw : raw.slice(0, split);
  const typeProvider = split === -1 ? (options.typeProvider ?? "") : raw.slice(split + 1);
  // Throws when the pair names no route, which is the validation.
  selectE2eRoutes({ fixture, typeProvider: typeProvider || undefined });
  return { fixture, typeProvider };
}

export function buildGitHubActionsMatrix(): {
  readonly include: Array<{
    readonly fixture: string;
    readonly type_provider: E2eTypeProviderRoute;
  }>;
} {
  return {
    include: buildRequiredE2eRouteInventory().map(({ fixture, typeProvider }) => ({
      fixture,
      type_provider: typeProvider,
    })),
  };
}
