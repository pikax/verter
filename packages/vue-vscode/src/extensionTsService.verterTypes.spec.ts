// The EXTENSION type provider must resolve `@verter/types`.
//
// Every IDE carrier Verter generates imports its type helpers from
// `@verter/types` (`Prettify`, `ExtractComponentProps`, `shallowUnwrapRef`, …).
// If that module does not resolve, TypeScript types every helper as the error
// `any`, so every template binding built through them degrades to `any` and the
// user gets a silently useless IDE — no error, just wrong answers.
//
// The three other provider routes already handle this (the tsserver plugin's
// virtual module, the managed-tsgo virtual `.d.ts`, the shared-tsgo adjacent
// overlay): detect whether the owning project resolves the package and, only
// when it does not, serve Verter's own declarations. This suite holds the
// extension route to the same contract, in both directions.
//
// These tests assert on the RESOLVED TYPE, never on the presence of an import or
// the absence of a diagnostic: `any` produces no diagnostic and keeps every
// import in place, so those signals cannot tell a working provider from a broken
// one.

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { ExtensionTsService } from "./extensionTsService.js";
import {
  materializeInstalledVerterTypes,
  materializeWorkspaceTypeScript,
} from "./extensionTsService.testUtils.js";

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

/**
 * A fixture project.
 *
 * `container` is the directory ABOVE the project root — where a hoisted
 * (monorepo / pnpm) dependency would live. It matters here: a dependency
 * installed there is resolvable by the normal ancestor walk but does NOT sit at
 * the path Verter's fallback occupies, which is exactly the placement that
 * distinguishes "real resolution ran first" from "the fallback always wins".
 */
function makeWorkspace(name: string): { root: string; container: string } {
  const container = mkdtempSync(join(tmpdir(), name));
  tmps.push(container);
  const root = join(container, "project");
  mkdirSync(root, { recursive: true });
  writeFileSync(join(root, "package.json"), JSON.stringify({ name: "fixture", private: true }));
  writeFileSync(
    join(root, "tsconfig.json"),
    JSON.stringify({
      compilerOptions: { module: "esnext", target: "esnext", moduleResolution: "bundler" },
      include: ["*.ts"],
    }),
  );
  return { root, container };
}

/** 1-based `{ line, offset }` of `needle`'s first occurrence, as tsserver counts. */
function positionOf(source: string, needle: string): { line: number; offset: number } {
  const index = source.indexOf(needle);
  if (index < 0) throw new Error(`fixture does not contain ${needle}`);
  const before = source.slice(0, index);
  const line = before.split("\n").length;
  const offset = index - (before.lastIndexOf("\n") + 1) + 1;
  return { line, offset };
}

/**
 * A probe shaped like the generated carrier surface: a binding whose type is
 * produced by `@verter/types` helpers. When the module resolves, `probe` is
 * `number`; when it does not, both helpers are the error `any` and so is `probe`.
 */
const PROBE_SOURCE = [
  'import type { ExtractComponentProps, Prettify } from "@verter/types";',
  "",
  "declare const component: { $props: { title: string; count: number } };",
  "type Props = Prettify<ExtractComponentProps<typeof component>>;",
  "declare const props: Props;",
  "export const probe = props.count;",
  "",
].join("\n");

function quickInfoForProbe(root: string): { kind: string; displayString: string } {
  const filePath = join(root, "entry.ts");
  writeFileSync(filePath, PROBE_SOURCE);

  const unavailable: string[] = [];
  const svc = new ExtensionTsService(root, (message) => unavailable.push(message));
  svc.handleQuery("open", { file: filePath, fileContent: PROBE_SOURCE, scriptKindName: "TS" });
  const where = positionOf(PROBE_SOURCE, "probe = props.count");
  const info = svc.handleQuery("quickinfo", { file: filePath, ...where }) as {
    kind: string;
    displayString: string;
  };
  expect(
    unavailable,
    "the fixture workspace has a real TypeScript; nothing may fail closed",
  ).toEqual([]);
  return info;
}

/**
 * Declarations for a fixture "installed" `@verter/types` that are deliberately
 * NOT what Verter ships: `Prettify` gains a member no Verter declaration has, and
 * `ExtractRenderComponent` (which Verter DOES ship) is absent entirely. Reading
 * the extra member proves the install was used; failing to resolve the missing
 * one proves the fallback did not quietly backfill it.
 */
const INSTALLED_DECLARATIONS = [
  "export type Prettify<T> = { installedMarker: T };",
  "export type ExtractComponentProps<T> = T;",
  "",
].join("\n");

/** Assert the INSTALLED declarations — not Verter's fallback — answered. */
function expectInstalledCopyWins(root: string): void {
  const filePath = join(root, "entry.ts");
  const source = [
    'import type { Prettify } from "@verter/types";',
    "",
    "declare const marked: Prettify<{ count: number }>;",
    "export const probe = marked.installedMarker;",
    "",
  ].join("\n");
  writeFileSync(filePath, source);

  const unavailable: string[] = [];
  const svc = new ExtensionTsService(root, (message) => unavailable.push(message));
  svc.handleQuery("open", { file: filePath, fileContent: source, scriptKindName: "TS" });
  const info = svc.handleQuery("quickinfo", {
    file: filePath,
    ...positionOf(source, "probe = marked.installedMarker"),
  }) as { displayString: string };
  expect(unavailable).toEqual([]);

  // `installedMarker` exists ONLY in the installed declarations; Verter's own
  // `Prettify` has no such member, so reading it proves the installed copy won.
  const oneLine = info.displayString.replace(/\s+/g, " ");
  expect(oneLine).toContain("{ count: number; }");
  expect(oneLine).not.toContain("any");

  // And the installed copy is the WHOLE authority: a name Verter ships but the
  // installed package does not declare must NOT be backfilled from the fallback,
  // or a version skew would silently answer from two merged packages.
  const skewSource = [
    'import type { ExtractRenderComponent } from "@verter/types";',
    "declare const x: ExtractRenderComponent<HTMLDivElement>;",
    "export const skew = x;",
    "",
  ].join("\n");
  const skewPath = join(root, "skew.ts");
  writeFileSync(skewPath, skewSource);
  svc.handleQuery("open", { file: skewPath, fileContent: skewSource, scriptKindName: "TS" });
  const skewDiagnostics = svc.handleQuery("semanticDiagnosticsSync", {
    file: skewPath,
  }) as Array<{ code: number }>;
  expect(
    skewDiagnostics.some((d) => d.code === 2305),
    "a member missing from the INSTALLED package must report, not resolve from Verter's fallback",
  ).toBe(true);
}

describe("ExtensionTsService — @verter/types resolution", () => {
  // The defect: with the package absent the provider had no fallback at all, so
  // the helper types were `any` and every binding built from them was `any`.
  it("resolves carrier type helpers to real types when @verter/types is NOT installed", () => {
    const { root } = makeWorkspace("ext-ts-verter-types-absent-");
    materializeWorkspaceTypeScript(root);
    // Deliberately NO materializeInstalledVerterTypes: this is a project that
    // never installed @verter/types, which is the normal case for a user who
    // only installed the extension.

    const info = quickInfoForProbe(root);

    expect(info.displayString).toContain("number");
    expect(
      info.displayString,
      "an `any` here is the silent failure this test exists to catch",
    ).not.toContain("any");
  });

  // Control A — the install sits exactly where the fallback would.
  it("uses the INSTALLED @verter/types in the project's own node_modules", () => {
    const { root } = makeWorkspace("ext-ts-verter-types-local-");
    materializeWorkspaceTypeScript(root);
    materializeInstalledVerterTypes(root, INSTALLED_DECLARATIONS);

    expectInstalledCopyWins(root);
  });

  // Control B — the DISCRIMINATING placement. A hoisted install (the normal
  // monorepo / pnpm layout) is resolvable by the ancestor walk but does not sit
  // at the fallback's path, so "always serve our own" and "resolve first, fall
  // back only on a miss" produce different answers here. Control A cannot tell
  // them apart, because there the two paths coincide.
  it("uses a HOISTED @verter/types from an ancestor, never the fallback", () => {
    const { root, container } = makeWorkspace("ext-ts-verter-types-hoisted-");
    materializeWorkspaceTypeScript(root);
    materializeInstalledVerterTypes(container, INSTALLED_DECLARATIONS);

    expectInstalledCopyWins(root);
  });
});
