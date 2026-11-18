import type { HostDependencyResolution, VerterHost } from "@verter/native";
import type { HostCanonicalId } from "../host-session/identity";
import type { PipelineCounters } from "./counters";

/**
 * `module`: how the bundler resolves the specifier at run time.
 * `declaration`: where the specifier's types are declared. Wins for the same specifier.
 */
export type RouteTier = "module" | "declaration";

interface OwnerRoutes {
  readonly module: Map<string, HostDependencyResolution>;
  readonly declaration: Map<string, HostDependencyResolution>;
}

/**
 * The single writer of each owner's import routes. Keeps the union of every
 * contribution per owner and pushes the whole union to the host after each
 * contribution, so the host's table never depends on which contributor ran last.
 *
 * Owners are host ids. Two spellings of one file must never become two owners.
 */
export class DependencyRouteTable {
  private readonly owners = new Map<HostCanonicalId, OwnerRoutes>();

  constructor(
    private readonly host: VerterHost,
    private readonly counters: PipelineCounters,
  ) {}

  /**
   * Merge `routes` into the owner's table under `tier` (a later route for the
   * same specifier in the same tier replaces the earlier one), then push the
   * union with one `setImportDependencies`. `always` pushes even when the
   * union is empty.
   */
  contribute(
    owner: HostCanonicalId,
    routes: readonly HostDependencyResolution[],
    tier: RouteTier,
    options?: { readonly always?: boolean },
  ): void {
    let held = this.owners.get(owner);
    if (!held) {
      held = { module: new Map(), declaration: new Map() };
      this.owners.set(owner, held);
    }
    for (const route of routes) held[tier].set(route.specifier, route);

    const union = this.routesOf(owner);
    if (union.length === 0 && !options?.always) return;
    this.host.setImportDependencies(owner, [...union]);
    this.counters.routePushes++;
  }

  /** The union currently held for `owner`, declaration tier winning. */
  routesOf(owner: HostCanonicalId): readonly HostDependencyResolution[] {
    const held = this.owners.get(owner);
    if (!held) return [];
    const union = new Map(held.module);
    for (const [specifier, route] of held.declaration) union.set(specifier, route);
    return [...union.values()];
  }

  /** Forget the owner's contributions. Does not call the host. */
  forget(owner: HostCanonicalId): void {
    this.owners.delete(owner);
  }

  clear(): void {
    this.owners.clear();
  }
}
