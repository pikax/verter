// @ai-generated - principal consumer of the 6-hop re-export chain.
// Imports `Foo` whose final definition (`{ b: 1 }`) lives 7 hops away in
// `mode_boundary_reexport_leaf.ts`. Mirrors the tsgo-audit benchmark
// re-export-chain shape.
//
// TS7 emission verified against tsgo 7.0.0-dev.20260523.1:
//   type WantedType = Foo & { a: 1 }
//   = { b: 1; } & { a: 1; } (structurally equivalent to `{ a: 1; b: 1 }`)
//   type WantedKeys = keyof WantedType
//   = "a" | "b"
import { Foo } from "./mode_boundary_reexport_link_1";

export type WantedType = Foo & { a: 1 };
export type WantedKeys = keyof WantedType;
