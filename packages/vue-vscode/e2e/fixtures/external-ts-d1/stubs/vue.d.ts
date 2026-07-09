// A minimal `vue` module stub (resolved via tsconfig `paths`) so the generated IDE
// carrier's `import("vue").*` references RESOLVE — a missing member would be TS2694,
// never the TS2307 the D1 acceptance forbids. Mirrors the hermetic stub the headless
// SHARED slice (`crates/verter_lsp/tests/shared_provider_live.rs`) uses.
export type PublicProps = {};
export type HTMLAttributes = Record<string, unknown>;
export type ShallowUnwrapRef<T> = T;
export type NativeElements = Record<string, unknown>;
export type GlobalDirectives = Record<string, unknown>;
export type Directive<T = any, V = any, M extends string = string> = unknown;
export declare const Comment: unique symbol;
export declare const Fragment: unique symbol;
export type Ref<T> = { value: T };
export type ExtractPropTypes<T> = T;
