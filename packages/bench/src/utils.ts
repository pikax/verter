import { basename } from "node:path";

export const ValidFiles = Object.entries(
  // @ts-expect-error not the correct flag on the tsconfig
  //   import.meta.glob("./../v5/parser/ast/__bench__/*.ts", {
  import.meta.glob("./__bench__/*.vue", {
    query: "?raw",
    eager: true,
    import: "default",
  }) as Record<string, string>
).reduce((c, [k, v]) => {
  c[basename(k)] = v;
  return c;
}, {} as Record<string, string>);
