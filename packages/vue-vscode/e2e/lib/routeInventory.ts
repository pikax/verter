export const TYPE_PROVIDER_ROUTES = ["tsserver", "tsgo", "shared-tsgo"] as const;

/**
 * Routes outside the standard fixture matrix. `editor-tsserver` is the explicit
 * editor-owned tier and `extension` the in-extension-host language service:
 * neither is ever selected automatically, so each is exercised only by the
 * acceptance fixture that owns it.
 */
export const NON_MATRIX_TYPE_PROVIDER_ROUTES = ["editor-tsserver", "extension", "off"] as const;

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
  "barrel-exports",
  "vue-contract",
  "svelte-contract",
  "vue-parity",
  "svelte-parity",
  "mixed-parity",
  "multi-root-parity",
  "ecosystem-parity",
] as const;

export interface E2eRoute {
  readonly fixture: string;
  readonly typeProvider: E2eTypeProviderRoute;
}

export const EDITOR_ACCEPTANCE_ROUTES: readonly E2eRoute[] = [
  { fixture: "editor-owned-project", typeProvider: "editor-tsserver" },
  { fixture: "editor-owned-project", typeProvider: "shared-tsgo" },
] as const;

/** Projectless fixtures exercise the intentional provider-off product surface. */
export const PROJECTLESS_E2E_ROUTES: readonly E2eRoute[] = [
  { fixture: "no-config", typeProvider: "off" },
  { fixture: "single-file", typeProvider: "off" },
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
  ...[
    "single-project",
    "monorepo",
    "tsconfig-extends",
    "tsconfig-references",
    "path-aliases",
    "composite-paths",
    "barrel-exports",
  ].map((fixture) => ({
    route: { fixture, typeProvider: "shared-tsgo" as const },
    reason:
      "the editor-owned shared-tsgo provider does not yet implement the legacy omnibus hover, completion, and navigation surface; editor-neutral and focused shared parity routes retain its lifecycle and topology coverage",
  })),
  ...["vue-contract", "svelte-contract"].map((fixture) => ({
    route: { fixture, typeProvider: "shared-tsgo" as const },
    reason:
      "the editor-owned shared-tsgo provider does not yet implement the complete framework contract; the managed tsserver and tsgo routes remain required for every contract row",
  })),
  {
    route: { fixture: "editor-owned-project", typeProvider: "shared-tsgo" },
    reason:
      "the shared-tsgo editor-owned route does not yet provide the typed hover and process-topology contract; the editor-tsserver acceptance and editor-neutral shared-provider contract remain required",
  },
  ...["tsserver", "tsgo"].map((typeProvider) => ({
    route: {
      fixture: "single-project",
      typeProvider: typeProvider as (typeof TYPE_PROVIDER_ROUTES)[number],
    },
    reason:
      "the legacy single-project omnibus mixes unfinished recovery and navigation features with regression checks; focused framework, barrel, project-topology, and parity contracts remain required",
  })),
  ...["vue-parity", "svelte-parity"].map((fixture) => ({
    route: { fixture, typeProvider: "shared-tsgo" as const },
    reason:
      "the full framework parity workload drives the incomplete editor-owned provider into managed fallback; shared-provider regressions remain gated by editor-neutral, mixed, multi-root, and ecosystem routes",
  })),
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
      TYPE_PROVIDER_ROUTES.map((typeProvider) => ({ fixture, typeProvider })),
    ),
    ...PROJECTLESS_E2E_ROUTES,
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

/**
 * Select an exact, ordered list of routes from a comma-separated list of
 * `<fixture>@<provider>` labels. This is the selector a CI shard runs with: the
 * shard planner emits the labels and the runner executes precisely those, so a
 * route can never be scheduled twice or dropped between the two. Explicit
 * labels reach the whole inventory, like `--fixture=<fixture>@<provider>`.
 */
export function selectE2eRoutesByLabels(labels: string): E2eRoute[] {
  const parsed = labels
    .split(",")
    .map((label) => label.trim())
    .filter((label) => label.length > 0);
  if (parsed.length === 0) {
    throw new Error("VS Code E2E route list must name at least one <fixture>@<provider> route");
  }
  const seen = new Set<string>();
  for (const label of parsed) {
    if (seen.has(label)) {
      throw new Error(`VS Code E2E route list names ${label} twice`);
    }
    seen.add(label);
  }
  return parsed.map(parseE2eRouteLabel);
}

/**
 * Resolve what one `runTests` invocation runs from its selectors. `E2E_ROUTES`
 * (a CI shard's exact list) is mutually exclusive with every other selector,
 * whichever form the other one takes — a labelled `--fixture=<f>@<p>` included —
 * so a stray environment variable can never silently override the list a
 * shard was handed, nor the other way round. Otherwise a labelled `--fixture`
 * names one route, and the fixture/provider selectors expand as before; no
 * selector at all is the required matrix.
 */
export function selectE2eRunRoutes(options: {
  readonly fixtureArg?: string;
  readonly envRoutes?: string;
  readonly envFixture?: string;
  readonly envTypeProvider?: string;
}): E2eRoute[] {
  if (options.envRoutes) {
    if (options.fixtureArg || options.envFixture || options.envTypeProvider) {
      throw new Error(
        "E2E_ROUTES cannot be combined with --fixture, E2E_FIXTURE or E2E_TYPE_PROVIDER",
      );
    }
    return selectE2eRoutesByLabels(options.envRoutes);
  }
  if (options.fixtureArg?.includes("@")) return [parseE2eRouteLabel(options.fixtureArg)];
  return selectE2eRoutes({
    fixture: options.fixtureArg ?? options.envFixture,
    typeProvider: options.envTypeProvider,
  });
}

/**
 * Relative per-route duration weights used ONLY to balance CI shards. A weight
 * is roughly the route's wall-clock minutes inside one launched VS Code host;
 * the parity workloads dominate, the topology fixtures sit in the middle, and
 * the contract, projectless and acceptance routes are the cheapest. Every
 * fixture in the inventory must appear here so a new fixture is weighted on
 * purpose rather than defaulting to a guess. Accuracy affects balance only,
 * never which routes run.
 */
export const E2E_FIXTURE_WEIGHTS: Readonly<Record<string, number>> = {
  "svelte-parity": 4.5,
  "vue-parity": 4,
  monorepo: 2,
  "tsconfig-extends": 2,
  "tsconfig-references": 2,
  "path-aliases": 2,
  "composite-paths": 2,
  "single-project": 1,
  "barrel-exports": 1,
  "vue-contract": 1,
  "svelte-contract": 1,
  "mixed-parity": 1,
  "multi-root-parity": 1,
  "ecosystem-parity": 1,
  "no-config": 1,
  "single-file": 1,
  "editor-owned-project": 1,
  "out-of-tree-monorepo": 1,
};

export function e2eRouteWeight(route: E2eRoute): number {
  const weight = E2E_FIXTURE_WEIGHTS[route.fixture];
  if (weight === undefined) {
    throw new Error(
      `VS Code E2E fixture ${JSON.stringify(route.fixture)} has no E2E duration weight; add it to E2E_FIXTURE_WEIGHTS`,
    );
  }
  return weight;
}

/**
 * How many CI runners the required matrix is spread across. Every shard pays
 * checkout, dependency install, artifact download and VS Code acquisition once,
 * then runs its routes back to back in one process; fewer, fuller shards cost
 * far less runner time and queue depth than one runner per route.
 */
export const E2E_CI_SHARD_COUNT = 6;

export interface E2eCiShard {
  /** One-based shard ordinal; the shard label is `${shard}/${shardCount}`. */
  readonly shard: number;
  readonly routes: readonly E2eRoute[];
}

/**
 * Partition the required matrix into `shardCount` duration-balanced shards.
 *
 * Longest-processing-time first: routes are taken heaviest first and each goes
 * onto the currently lightest shard, ties broken by shard ordinal. The input
 * order is the inventory order with a stable weight sort, so the partition is a
 * pure function of the inventory and the weight table. Every required route
 * lands in exactly one shard; deselected routes are never scheduled.
 */
export function buildE2eCiShards(shardCount: number = E2E_CI_SHARD_COUNT): E2eCiShard[] {
  const required = buildRequiredE2eRouteInventory();
  if (!Number.isInteger(shardCount) || shardCount < 1 || shardCount > required.length) {
    throw new Error(
      `VS Code E2E shard count must be an integer in 1..${required.length}, got ${shardCount}`,
    );
  }
  const ordered = required
    .map((route, index) => ({ route, index, weight: e2eRouteWeight(route) }))
    .sort((a, b) => b.weight - a.weight || a.index - b.index);
  const loads = new Array<number>(shardCount).fill(0);
  const routes: E2eRoute[][] = Array.from({ length: shardCount }, () => []);
  for (const { route, weight } of ordered) {
    let lightest = 0;
    for (let shard = 1; shard < shardCount; shard++) {
      if (loads[shard] < loads[lightest]) lightest = shard;
    }
    loads[lightest] += weight;
    routes[lightest].push(route);
  }
  return routes.map((shardRoutes, index) => ({ shard: index + 1, routes: shardRoutes }));
}

/**
 * The GitHub Actions matrix: one entry per CI shard. `routes` is the exact
 * comma-separated route list the runner receives through `E2E_ROUTES`, so the
 * workflow never re-derives which routes a shard owns.
 */
export function buildGitHubActionsMatrix(): {
  readonly include: Array<{
    readonly shard: string;
    readonly shard_count: string;
    readonly routes: string;
  }>;
} {
  const shards = buildE2eCiShards();
  return {
    include: shards.map(({ shard, routes }) => ({
      shard: String(shard),
      shard_count: String(shards.length),
      routes: routes.map(e2eRouteLabel).join(","),
    })),
  };
}
