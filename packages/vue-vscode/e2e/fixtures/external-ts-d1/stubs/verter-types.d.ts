// A minimal `@verter/types` module stub (resolved via tsconfig `paths`) — the IDE
// carrier helper surface, declared loosely so `import from "@verter/types"` resolves
// cleanly (missing members would be TS2305, never the forbidden TS2307). Mirrors the
// hermetic stub the headless SHARED slice uses.
export type Prettify<T> = { [K in keyof T]: T[K] };
export type ExtractComponentProps<T> = Record<string, unknown>;
export type ExtractLeafElement<T> = unknown;
export declare function shallowUnwrapRef<T>(value: T): T;
export declare function enhanceElementWithProps<T, P>(el: T, props: P): T;
export declare function extractRenderComponent<T>(t: T): unknown;
export declare function instantiateComponent<T>(t: T): unknown;
export declare function extractArgumentsFromRenderSlot(...args: unknown[]): unknown;
export declare function runCustomDirective(...args: unknown[]): unknown;
export declare function retrieveSetupDirectives<T>(instance: T): unknown;
export declare function strictRenderSlot(...args: unknown[]): unknown;
export declare function checkRequiredSlots(...args: unknown[]): unknown;
