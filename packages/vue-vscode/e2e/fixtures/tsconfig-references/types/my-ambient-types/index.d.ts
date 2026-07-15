// Ambient global reached only through tsconfig `types` + `typeRoots`. Resolves
// only when the LEAF configured project (`tsconfig.app.json`) is loaded.
declare global {
  const AMBIENT_TOKEN: "ambient-token-value";
}
export {};
