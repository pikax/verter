// Ambient global reached only through the app leaf's tsconfig `types` +
// `typeRoots`. Resolves only when the app's leaf configured project is loaded.
declare global {
  const AMBIENT_TOKEN: "ambient-token-value";
}
export {};
