// Hermetic minimal `vue` type shim for the DX differential baseline.
//
// Committed (never installed at runtime) and pinned to the workspace Vue line.
// Self-contained: it references no `@vue/*` package, so the baseline provider
// resolves it without pulling a transitive runtime declaration graph. The
// surface is deliberately small — the differential gates type identity through
// the materialized `.vue.tsx`, not through this shim's strictness.

export type Ref<T = unknown> = { value: T };
export type ComputedRef<T = unknown> = { readonly value: T };
export type Reactive<T extends object> = T;

export declare function ref<T>(value: T): Ref<T>;
export declare function ref<T = unknown>(): Ref<T | undefined>;
export declare function computed<T>(getter: () => T): ComputedRef<T>;
export declare function reactive<T extends object>(target: T): Reactive<T>;
export declare function watch(source: unknown, cb: (...args: unknown[]) => void): () => void;
export declare function watchEffect(effect: () => void): () => void;
export declare function onMounted(hook: () => void): void;
export declare function onUnmounted(hook: () => void): void;
export declare function nextTick(): Promise<void>;
export declare function h(type: unknown, props?: unknown, children?: unknown): unknown;

// `<script setup>` fallthrough-attribute accessor: the inherited HTML attributes a
// component did not declare as props. Loosely typed here (the differential gates
// the precise shape through the materialized `.vue.tsx`, not this shim).
export type ComponentAttrs = Record<string, unknown>;
export declare function useAttrs(): ComponentAttrs;

export type DefineComponent<Props = Record<string, unknown>> = new () => { $props: Props };
export declare function defineComponent<Props = Record<string, unknown>>(
  options: unknown,
): DefineComponent<Props>;
