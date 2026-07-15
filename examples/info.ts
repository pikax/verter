/**
 * Quick playground for @verter/typeinfo against an in-memory scratch
 * file plus one real file from examples/src/.
 *
 * Run:
 *   pnpm --filter examples exec tsx info.ts        # if tsx is available
 *   node --experimental-strip-types examples/info.ts   # Node 22+
 *
 * Edit SCRATCH_SRC, the resolveSymbol / evaluateTypeExpression calls,
 * or the printer to inspect whatever you're poking at.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

import {
  TypeInfoSession,
  type ProjectionMode,
  type ResolveSymbolResult,
  type EvaluateTypeExpressionResult,
  type TypeDescriptor,
} from "@verter/typeinfo";

const here = dirname(fileURLToPath(import.meta.url));

const SCRATCH_ID = resolve(here, "src/_scratch-typeinfo.ts");
const SCRATCH_SRC =
  `
export interface User {
  id: number;
  name: string;
  email?: string;
}

export type UserKey = keyof User;

export type Box<T> = { value: T };

export type Public<T> = Omit<T, "email">;


export type MyType = Pick<User, "id" | "name"> | Box<string>["value"] | User["id"] | User["name"];

interface NotExported {
  secret: string;
}


interface CircularA extends NotExported {
  b: CircularA;
}

const Foo: CircularA;
` +
  Array.from({ length: 10000 })
    .map((_, i) =>
      i === 0
        ? `type Var0 = { a: 1 }`
        : `type Var${i} = ${Array.from({ length: i })
            .map((_, b) => `Var${b}`)
            .join("&")}`,
    )
    .join("\n");

let measure = performance.measure("foo");

console.log("started");
const REAL_FILE = resolve(here, "src/props/types.ts");

function describe(descriptor: TypeDescriptor | undefined): string {
  if (!descriptor) return "(none)";
  if (descriptor.kind === "primitive") return `primitive(${descriptor.name})`;
  if (descriptor.kind === "literal") return `literal(${JSON.stringify(descriptor.value)})`;
  if (descriptor.kind === "object") {
    const fields = descriptor.properties
      .map((p) => `${p.name}${p.optional ? "?" : ""}: ${describe(p.type)}`)
      .join(", ");
    return `object({ ${fields} })`;
  }
  if (descriptor.kind === "union") return `union(${descriptor.types.map(describe).join(" | ")})`;
  if (descriptor.kind === "intersection")
    return `intersection(${descriptor.types.map(describe).join(" & ")})`;
  if (descriptor.kind === "array") return `array(${describe(descriptor.element)})`;
  if (descriptor.kind === "ref") return `ref(${descriptor.name})`;
  if (descriptor.kind === "indexedAccess")
    return `indexedAccess(${describe(descriptor.objectType)}[${describe(descriptor.indexType)}])`;
  if (descriptor.kind === "recursiveRef")
    return `recursiveRef(${descriptor.conditionalContext}${descriptor.name})`;
  return descriptor.kind;
}

function summariseAudit(result: ResolveSymbolResult | EvaluateTypeExpressionResult) {
  if (!result.auditRecord) return "(no audit)";
  const a = result.auditRecord;
  return `kind=${a.kind} fromCache=${a.from_cache ?? "?"} totalMs=${a.timings?.total_ms ?? "?"}`;
}

function tryResolve(
  session: TypeInfoSession,
  file: string,
  name: string,
  mode?: ProjectionMode,
): void {
  const result = session.resolveSymbol(file, name, mode ? { mode } : undefined);
  const tag = mode ? `${name} [${mode}]` : `${name}`;
  console.log(`  ${tag.padEnd(30)} -> ${describe(result.type)}`);
  console.log(`  ${"".padEnd(30)}    audit: ${summariseAudit(result)}`);
}

function play(): void {
  const session = new TypeInfoSession({ root: here });

  try {
    // --- inline scratch ---
    session.host.upsert({ inputId: SCRATCH_ID, source: SCRATCH_SRC });

    console.log("=== listSymbols(scratch) ===");
    // for (const sym of session.listSymbols(SCRATCH_ID)) {
    //   console.log(
    //     `  ${sym.name.padEnd(18)} ${sym.kind.padEnd(14)} exported=${sym.isExported}`,
    //   );
    // }

    console.log("\n=== resolveSymbol(scratch) ===");
    tryResolve(session, SCRATCH_ID, "User");
    tryResolve(session, SCRATCH_ID, "User", "identity");
    tryResolve(session, SCRATCH_ID, "User", "expanded");
    tryResolve(session, SCRATCH_ID, "UserKey", "expanded");
    tryResolve(session, SCRATCH_ID, "Public", "expanded");
    tryResolve(session, SCRATCH_ID, "Box");
    tryResolve(session, SCRATCH_ID, "DoesNotExist");
    tryResolve(session, SCRATCH_ID, "MyType", "expanded");
    tryResolve(session, SCRATCH_ID, "CircularA", "expanded");
    tryResolve(session, SCRATCH_ID, "Foo", "expanded");

    // --- evaluateTypeExpression ---
    console.log("\n=== evaluateTypeExpression(scratch scope) ===");
    const cases = [
      `Pick<User, "id" | "name">`,
      `Box<string>["value"]`,
      `User["id"] | User["name"]`,
    ];
    for (const expression of cases) {
      const result = session.evaluateTypeExpression({
        scope: SCRATCH_ID,
        expression,
        mode: "expanded",
      });
      console.log(`  ${expression.padEnd(34)} -> ${describe(result.type)}`);
      console.log(`  ${"".padEnd(34)}    audit: ${summariseAudit(result)}`);
    }

    // --- a real file from examples/src/ ---
    console.log("\n=== real file: src/props/types.ts ===");
    session.host.upsert({ inputId: REAL_FILE, source: readFileSync(REAL_FILE, "utf-8") });
    console.log("  symbols:");
    for (const sym of session.listSymbols(REAL_FILE)) {
      console.log(`    ${sym.name.padEnd(16)} ${sym.kind.padEnd(14)} exported=${sym.isExported}`);
    }
    tryResolve(session, REAL_FILE, "MyProps", "expanded");
  } finally {
    session.host.close();
    console.log("terminated", measure.toJSON());
  }
}

play();
