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
export type Unrelated = { a: 1 };
` +
  Array.from({ length: 10000 })
    .map((_, i) =>
      i === 0
        ? `type Var0 = { a: 1 }`
        : `type Var${i} = ${Array.from({ length: i })
            .map((_, b) => `Var${b}`)
            .join("&")}`,
    )
    .join(";\n");

const start = performance.now();

console.log("started");
const session = new TypeInfoSession({ root: here });
session.host.upsert({ inputId: SCRATCH_ID, source: SCRATCH_SRC });

// resolve unrelated
const unrelated = session.resolveSymbol(SCRATCH_ID, "Unrelated", { mode: "expanded" });

console.log("end", performance.now() - start, "ms");
