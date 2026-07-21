// Ambient global reached only through tsconfig `types` + `typeRoots`. Resolves
// only when a leaf configured project (a `tsconfig.*.json` leaf) is loaded and
// owns the carrier.
declare global {
  const AMBIENT_TOKEN: "ambient-token-value";
}
export {};
