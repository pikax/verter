import type { HostCompileVirtualNode } from "@verter/native";
import type { VerterRenderProfile } from "../compiler";
import type {
  BundlerGraphKind,
  BundlerModuleId,
  BundlerModuleRole,
  BundlerSourceRevision,
  CarrierFramework,
  HostCanonicalId,
} from "./identity";
import { BUNDLER_GRAPH_KINDS, encodeFramed, roleKey } from "./identity";

export interface PublishedMain {
  readonly code: string;
  readonly map: string | null;
  /** `ts`, `js`, `jsx`, … as the compiler stated it. */
  readonly lang: string | null;
}

/** One authored `<style>` block, for the lane where the bundler's CSS pipeline processes it. */
export interface AuthoredStyleBlock {
  readonly blockToken: string;
  readonly content: string;
  readonly lang: string;
  readonly scoped: boolean;
  /** `false`, `true` (`$style`), or the exposed name. */
  readonly module: boolean | string;
}

/** One compiled, scoped style, for the lane where this package serves final CSS. */
export interface CompiledStyle {
  readonly code: string;
  readonly map: string | null;
  readonly lang: string;
}

/** Everything published for one carrier in one graph. Immutable once published. */
export interface CarrierRecord {
  readonly carrier: HostCanonicalId;
  /** The path spelling the bundler used; request ids are built from it. */
  readonly rawPath: string;
  readonly graph: BundlerGraphKind;
  readonly framework: CarrierFramework;
  readonly revision: BundlerSourceRevision;
  /** The session epoch the record was produced under. */
  readonly epoch: number;
  readonly profile: VerterRenderProfile;
  readonly main: PublishedMain | null;
  readonly authoredStyles: readonly AuthoredStyleBlock[];
  /** Indexed by style block ordinal; no holes. */
  readonly compiledStyles: readonly CompiledStyle[];
  /** Compiled nodes that are neither main nor style (template, custom blocks). */
  readonly nodes: readonly HostCompileVirtualNode[];
  /** Host ids of every hydrated dependency file the compile read. */
  readonly closure: ReadonlySet<HostCanonicalId>;
}

export interface CatalogModule {
  readonly id: BundlerModuleId;
  readonly role: BundlerModuleRole;
  readonly record: CarrierRecord;
}

/** The only store of published modules of one session. */
export class ModuleCatalog {
  private readonly records = new Map<string, CarrierRecord>();
  /** The modules each record publishes, rebuilt whenever that record is written. */
  private readonly modulesByKey = new Map<string, readonly CatalogModule[]>();
  private readonly modulesById = new Map<BundlerModuleId, CatalogModule>();

  constructor(
    private readonly mint: (record: CarrierRecord, role: BundlerModuleRole) => BundlerModuleId,
  ) {}

  /** Replace the record of `(record.carrier, record.graph)` in one step. */
  publish(record: CarrierRecord): void {
    this.write(record);
  }

  record(carrier: HostCanonicalId, graph: BundlerGraphKind): CarrierRecord | undefined {
    return this.records.get(keyOf(carrier, graph));
  }

  /** Every module the record of `(carrier, graph)` publishes: main, script, each style, each other node. */
  modules(carrier: HostCanonicalId, graph: BundlerGraphKind): readonly CatalogModule[] {
    return this.modulesByKey.get(keyOf(carrier, graph)) ?? [];
  }

  module(id: BundlerModuleId): CatalogModule | undefined {
    return this.modulesById.get(id);
  }

  /** Remove the carrier's records in every graph. Returns how many were removed. */
  retire(carrier: HostCanonicalId): number {
    let removed = 0;
    for (const graph of BUNDLER_GRAPH_KINDS) {
      if (this.drop(keyOf(carrier, graph))) removed += 1;
    }
    return removed;
  }

  /** Remove every record whose closure contains `dependency`. Returns the carriers removed. */
  retireDependents(dependency: HostCanonicalId): readonly HostCanonicalId[] {
    const doomed: CarrierRecord[] = [];
    for (const record of this.records.values()) {
      if (record.closure.has(dependency)) doomed.push(record);
    }
    const carriers = new Set<HostCanonicalId>();
    for (const record of doomed) {
      this.drop(keyOf(record.carrier, record.graph));
      carriers.add(record.carrier);
    }
    return [...carriers];
  }

  clear(): void {
    this.records.clear();
    this.modulesByKey.clear();
    this.modulesById.clear();
  }

  /** Number of records. */
  size(): number {
    return this.records.size;
  }

  private write(record: CarrierRecord): void {
    const key = keyOf(record.carrier, record.graph);
    this.removeModules(key);
    this.records.set(key, record);
    const modules = listModules(record).map((role) => ({
      id: this.mint(record, role),
      role,
      record,
    }));
    this.modulesByKey.set(key, modules);
    for (const entry of modules) this.modulesById.set(entry.id, entry);
  }

  private drop(key: string): boolean {
    this.removeModules(key);
    return this.records.delete(key);
  }

  private removeModules(key: string): void {
    for (const entry of this.modulesByKey.get(key) ?? []) this.modulesById.delete(entry.id);
    this.modulesByKey.delete(key);
  }
}

function keyOf(carrier: HostCanonicalId, graph: BundlerGraphKind): string {
  return encodeFramed([carrier, graph]);
}

/** The roles a record publishes, each once, in a stable order. */
function listModules(record: CarrierRecord): BundlerModuleRole[] {
  const roles: BundlerModuleRole[] = [{ kind: "main" }];
  if (record.main) roles.push({ kind: "script" });
  const styleCount = Math.max(record.authoredStyles.length, record.compiledStyles.length);
  for (let index = 0; index < styleCount; index += 1) roles.push({ kind: "style", index });
  for (const node of record.nodes) {
    const role = roleOfNode(node);
    if (role) roles.push(role);
  }
  const seen = new Set<string>();
  return roles.filter((role) => {
    const key = roleKey(role);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function roleOfNode(node: HostCompileVirtualNode): BundlerModuleRole | null {
  const { kind, index } = node.node;
  switch (kind) {
    case "main":
      return { kind: "main" };
    case "script":
      return { kind: "script" };
    case "template":
      return { kind: "template" };
    case "style":
      return { kind: "style", index: index ?? 0 };
    case "custom":
      return { kind: "custom", index: index ?? 0, blockType: node.meta?.blockType ?? "" };
    default:
      return null;
  }
}
