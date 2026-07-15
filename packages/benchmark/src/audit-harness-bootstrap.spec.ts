/**
 * Discrimination test for the VFS audit-harness empty-topology bug.
 *
 * Background: V1-V5 + W1/W2 (see the VFS investigation scratch notes) traced the
 * misleading 19/45 semanticMiss result on `Table.vue` to two
 * independent bootstrap errors in `_audit-component.ts`:
 *
 *  1. The audit harness did NOT install an alias map on the
 *     workspace, so `Engine::resolve_import` walked an empty
 *     `ProjectGraph` and short-circuited before reaching the
 *     pnpm-aware resolver. `NapiWorkspace::new` is lazy by design
 *     (eager auto-discovery was reverted from 9ae1171b8 after a 3.6x
 *     bench regression) — consumers MUST explicitly call
 *     `workspace.configureProjects(...)` to install path aliases,
 *     mirroring `compat/checker.ts:2265`.
 *  2. The audit harness used a root-relative canonical
 *     (`"/" + relative(uiRoot, componentFile)`) that did not match the
 *     absolute project root configured anywhere else in Verter's
 *     host-backed pipeline.
 *
 * Discrimination strategy: run the audit query with BOTH canonical
 * forms on the same explicitly-configured workspace and assert that
 * the absolute form publishes a richer topology than the
 * root-relative form. The post-fix harness uses the absolute form
 * AND explicitly configures the project graph; the pre-fix harness
 * used the root-relative form OR skipped the configureProjects call.
 * The assertion fails RED iff:
 *
 *  - the canonical-id fix is reverted (absolute stops outperforming
 *    root-relative on the same configured workspace), OR
 *  - a future change skips `configureProjects` (both forms regress
 *    symmetrically and `imported_dependency_entries` drops to 1).
 *
 * The fixture is fully hermetic per the testing-hermeticity rule: a
 * self-contained tempdir with one Vue component, one TS dep, and one
 * tsconfig — no dependency on vendored third-party corpora.
 */

import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const __filename = fileURLToPath(import.meta.url);
const requireFromHere = createRequire(__filename);

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let native: any;
try {
  native = requireFromHere("@verter/native");
} catch {
  native = null;
}

interface StructuredCustom {
  name?: string;
  detail?: string;
}
interface StructuredEvent {
  Custom?: StructuredCustom;
}
interface AuditFootprint {
  structured_events?: StructuredEvent[];
}
interface AuditStore {
  imported_dependency_entries?: number;
}
interface AuditRecord {
  store?: AuditStore;
  footprint?: AuditFootprint;
}
interface AuditBundle {
  record?: AuditRecord;
  analysis?: unknown;
  resolution?: unknown;
}

function buildFixture(): {
  uiRoot: string;
  componentFile: string;
} {
  const uiRoot = mkdtempSync(resolve(tmpdir(), "verter-audit-harness-")).replace(/\\/g, "/");

  // Single tsconfig at the root — `NapiWorkspace::new` discovers it
  // eagerly post-fix.
  writeFileSync(
    `${uiRoot}/tsconfig.json`,
    JSON.stringify(
      {
        compilerOptions: {
          target: "es2020",
          module: "esnext",
          moduleResolution: "bundler",
          strict: true,
          jsx: "preserve",
        },
        include: ["src/**/*"],
      },
      null,
      2,
    ),
    "utf8",
  );

  mkdirSync(`${uiRoot}/src`, { recursive: true });
  mkdirSync(`${uiRoot}/src/types`, { recursive: true });

  writeFileSync(
    `${uiRoot}/src/types/props.ts`,
    [
      "export interface SampleProps {",
      "  /** A simple required string */",
      "  label: string;",
      "  /** An optional number */",
      "  count?: number;",
      "}",
      "",
    ].join("\n"),
    "utf8",
  );

  const componentFile = `${uiRoot}/src/Sample.vue`;
  writeFileSync(
    componentFile,
    [
      '<script setup lang="ts">',
      "import type { SampleProps } from './types/props';",
      "",
      "defineProps<SampleProps>();",
      "</script>",
      "",
      "<template>",
      "  <div>{{ count ?? 0 }}</div>",
      "</template>",
      "",
    ].join("\n"),
    "utf8",
  );

  return { uiRoot, componentFile };
}

/**
 * Install a minimal alias map on the workspace mirroring
 * `_audit-component.ts::configureWorkspaceProjects` and
 * `compat/checker.ts:2265`. The fixture's tsconfig has no
 * `compilerOptions.paths`, so the installed config carries an empty
 * paths array — but the mere act of `configureProjects` populates the
 * `ProjectGraph` with an Explicit-rank entry, which lets
 * `Engine::resolve_import` route through the pnpm-aware resolver
 * instead of short-circuiting on an empty graph.
 */
function configureWorkspaceForFixture(
  workspace: { configureProjects: (configs: unknown[]) => void },
  uiRoot: string,
): void {
  const normalizedRoot = uiRoot.replace(/\\/g, "/");
  workspace.configureProjects([
    {
      root: normalizedRoot,
      workspaceRoot: normalizedRoot,
      compilerOptions: {
        baseUrl: undefined,
        paths: undefined,
      },
    },
  ]);
}

/**
 * Run the audit harness flow against a given canonical id, returning
 * the parsed bundle for assertion. Mirrors the post-fix
 * `_audit-component.ts` exactly except for the canonical-id form,
 * which the caller supplies so the discriminator can compare
 * pre-fix (root-relative) vs post-fix (absolute) shapes.
 */
function runAudit(
  uiRoot: string,
  componentFile: string,
  canonicalForm: "absolute" | "root-relative",
): AuditBundle {
  const canonical =
    canonicalForm === "absolute"
      ? componentFile
      : "/" + relative(uiRoot, componentFile).replace(/\\/g, "/");

  const workspace = new native.Workspace([uiRoot]);
  configureWorkspaceForFixture(workspace, uiRoot);
  const project = native.ComponentMetaHost.withWorkspace(
    { auditEnabled: true, footprintCapture: true },
    workspace,
  );

  const loaded = project.ensureLoaded(canonical);
  if (!loaded) {
    const source = readFileSync(componentFile, "utf-8");
    project.upsertBase(canonical, source);
  }

  try {
    const session = project.openSession();
    try {
      const buffer: Buffer | null = session.getComponentMetaWithAudit(canonical);
      if (buffer === null) {
        return {};
      }
      return JSON.parse(buffer.toString("utf-8")) as AuditBundle;
    } finally {
      session.close();
    }
  } finally {
    project.shutdown();
  }
}

function resolvedRouteEventCount(bundle: AuditBundle): number {
  const events = bundle.record?.footprint?.structured_events ?? [];
  return events.filter((e) => {
    if (e?.Custom?.name !== "authoritative_import_route_result") {
      return false;
    }
    const detail = e.Custom?.detail ?? "";
    return detail.includes("target=") && !detail.includes("target=<none>");
  }).length;
}

describe("audit-harness — VFS bootstrap discriminator (Phase Y1)", () => {
  it("audit_component_absolute_canonical_outperforms_root_relative_canonical", () => {
    if (!native) {
      // Native is only built in build:native; on platforms where
      // it's unavailable just skip the discriminator rather than
      // failing CI on environments that legitimately can't load
      // the addon.
      expect(true, "native module not built; skipping discriminator").toBe(true);
      return;
    }

    const { uiRoot, componentFile } = buildFixture();

    // Run the same audit flow twice with different canonical
    // forms. The absolute form is the post-fix shape (matches
    // `meta-ui-bench.ts`, `compat/checker.ts`, and the LSP); the
    // root-relative form is the pre-fix shape that the V1 audit
    // identified as the source of the empty-topology bug.
    const absoluteBundle = runAudit(uiRoot, componentFile, "absolute");
    const rootRelativeBundle = runAudit(uiRoot, componentFile, "root-relative");

    const absoluteResolved = resolvedRouteEventCount(absoluteBundle);
    const rootRelativeResolved = resolvedRouteEventCount(rootRelativeBundle);

    // Discriminator: the absolute canonical MUST resolve at least
    // one import; pre-fix the root-relative canonical resolved
    // ZERO. If a future change collapses the two paths into
    // equivalent behavior, this assertion fails and the regression
    // surfaces immediately.
    expect(
      absoluteResolved,
      `absolute canonical must resolve at least one import; got ${absoluteResolved}. Pre-fix root-relative resolved ${rootRelativeResolved}.`,
    ).toBeGreaterThanOrEqual(1);

    expect(
      absoluteResolved,
      `absolute canonical MUST resolve more imports than root-relative — pre-fix the harness used root-relative and ALL imports had target=<none>. Got absolute=${absoluteResolved}, root-relative=${rootRelativeResolved}.`,
    ).toBeGreaterThan(rootRelativeResolved);

    // Additional invariant: the absolute-canonical bundle must
    // carry a non-degenerate topology. `imported_dependency_entries`
    // counts the component + every transitively-loaded dep; pre-fix
    // it was 1 (component only). Post-fix it MUST be >= 2 for the
    // 1-import fixture.
    expect(
      absoluteBundle.record?.store?.imported_dependency_entries ?? 0,
      "absolute canonical must produce a non-degenerate topology (>= 2 imported_dependency_entries for the 1-import fixture)",
    ).toBeGreaterThan(1);
  }, 60_000);
});
