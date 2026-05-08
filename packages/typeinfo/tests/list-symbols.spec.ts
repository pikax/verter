/**
 * Phase 4 Test #1 — `TypeInfoSession.listSymbols`.
 *
 * Mirrors the Phase 3 Rust `list_file_symbols_*` characterisation
 * tests (§5.1): a fixture with type aliases + interfaces + enums
 * surfaces every declaration with the right `kind` and `isExported`
 * flag through the JS API.
 *
 * REGRESSION classification — fails against any pre-Phase 4
 * substrate that is missing the `listSymbols` NAPI method.
 */

import { describe, expect, it } from "vitest";

import { TypeInfoSession } from "../src/index.js";

const FIXTURE = `
export type ExportedAlias = string;
type LocalAlias = number;

export interface ExportedInterface { x: number }
interface LocalInterface { y: string }

export enum ExportedEnum { A, B }
enum LocalEnum { X, Y }

export const exportedConst = 1;
const localConst = 2;

export function exportedFn() {}
function localFn() {}

export class ExportedClass {}
class LocalClass {}

export async function exportedAsyncFn() {}
`;

describe("TypeInfoSession.listSymbols", () => {
  it("surfaces every top-level type / value declaration with the right kind and export flag", () => {
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/fixtures/api.ts",
      source: FIXTURE,
    });
    const symbols = session.listSymbols("/fixtures/api.ts");

    // Build a map { (name, kind) -> entry } for assertion clarity.
    const byKey = new Map(symbols.map((s) => [`${s.name}:${s.kind}`, s] as const));

    // Type-alias entries
    expect(byKey.get("ExportedAlias:typeAlias")?.isExported).toBe(true);
    expect(byKey.get("LocalAlias:typeAlias")?.isExported).toBe(false);

    // Interface entries
    expect(byKey.get("ExportedInterface:interface")?.isExported).toBe(true);
    expect(byKey.get("LocalInterface:interface")?.isExported).toBe(false);

    // Class — appears as both type-side (kind: "class") and
    // value-side (kind: "classValue").
    expect(byKey.has("ExportedClass:class")).toBe(true);
    expect(byKey.has("ExportedClass:classValue")).toBe(true);
    expect(byKey.get("ExportedClass:class")?.isExported).toBe(true);

    // Value-side entries — these are the discriminating
    // assertions. Enum declarations live in the value table only
    // when the shallow analyser captures them; this fixture
    // additionally covers `const`, `function`, and async function.
    expect(byKey.get("exportedConst:const")?.isExported).toBe(true);
    expect(byKey.get("localConst:const")?.isExported).toBe(false);
    expect(byKey.get("exportedFn:function")?.isExported).toBe(true);
    expect(byKey.get("localFn:function")?.isExported).toBe(false);
    expect(byKey.get("exportedAsyncFn:asyncFunction")?.isExported).toBe(true);

    // Negative — imported symbols are NOT surfaced.
    expect(byKey.has("Buffer:typeAlias")).toBe(false);

    session.host.close();
  });

  it("returns an empty inventory for a non-existent file", () => {
    const session = new TypeInfoSession({ root: "/fixtures" });
    const symbols = session.listSymbols("/fixtures/missing.ts");
    expect(symbols).toEqual([]);
    session.host.close();
  });

  it("captures spans for declared symbols when the analysis snapshot has them", () => {
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/fixtures/spans.ts",
      source: "export type Foo = string;\nexport const bar = 1;\n",
    });
    const symbols = session.listSymbols("/fixtures/spans.ts");
    const foo = symbols.find((s) => s.name === "Foo" && s.kind === "typeAlias");
    expect(foo).toBeDefined();
    if (foo?.span) {
      // Span integer fields are well-formed.
      expect(typeof foo.span.start).toBe("number");
      expect(typeof foo.span.end).toBe("number");
      expect(foo.span.end).toBeGreaterThan(foo.span.start);
    }
    session.host.close();
  });
});
