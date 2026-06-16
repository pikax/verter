/**
 * Runtime ESM dynamic import that survives a CommonJS `tsc` emit.
 *
 * The DX modules under `e2e/dx` compile to CommonJS (`tsconfig.test.json`), but
 * `@verter/dx-harness` is ESM-only (`"type": "module"`, import/default-only
 * exports). A literal `await import(x)` written in CJS-targeted TypeScript is
 * DOWNLEVELED by the compiler to `require(x)` (`Promise.resolve().then(() =>
 * __importStar(require(x)))`), which throws `ERR_REQUIRE_ESM` against an ESM
 * package — the launcher would die before VS Code ever starts.
 *
 * Building the dynamic import from a STRING via `Function` hides the `import(...)`
 * syntax from the TypeScript module transform, so it is emitted verbatim and Node
 * performs a genuine ESM dynamic import at runtime. This is the single mechanism the
 * Node-side launcher (the `@verter/dx-harness` barrel) and the in-host startup gate
 * (the `@verter/dx-harness/startup-gate` subpath) both use to reach the ESM harness.
 */

// `import(specifier)` lives inside a string literal, so the TS CommonJS transform
// never sees it and cannot rewrite it to `require`. The emitted call is a real
// dynamic `import()` — the property this module exists to guarantee.
// eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
const dynamicImport = new Function("specifier", "return import(specifier);") as (
  specifier: string,
) => Promise<unknown>;

/**
 * Load an ESM module by specifier at runtime, regardless of the caller's module
 * system. Returns the module namespace. Never collapses to `require`, so it works
 * from CommonJS-compiled code against ESM-only packages.
 */
export function importEsm<T = unknown>(specifier: string): Promise<T> {
  return dynamicImport(specifier) as Promise<T>;
}
