/**
 * Template condition narrowing helpers for Verter's generated IDE TSX.
 *
 * TypeScript narrows a reference only along one function's control flow, and
 * a callback starts its flow from the declared type of every property access.
 * The generated TSX therefore snapshots each outer reference a template
 * callback reads in the narrowed flow (`const o = state.u;`) and re-narrows it
 * at the callback body's start:
 *
 * ```ts
 * if (!flowNarrow(state.u, o) || flowExcluded(o)(state.u)) throw 0;
 * ```
 */

/** Whether `A` and `B` are the same type. */
export type FlowSame<A, B> =
  (<G>() => G extends A ? 1 : 2) extends <G>() => G extends B ? 1 : 2 ? true : false;

/** `true` when some constituent of `S` is exactly `R`. */
export type FlowKept<R, S> = S extends unknown
  ? FlowSame<R, S> extends true
    ? true
    : never
  : never;

/** The constituents of `R` that are not exactly a constituent of `S`. */
export type FlowExcluded<R, S> = R extends unknown
  ? [FlowKept<R, S>] extends [never]
    ? R
    : never
  : never;

/** Narrows `reference` to the type of its `snapshot`. */
export declare function flowNarrow<S>(reference: unknown, snapshot: S): reference is S;

/**
 * Removes the constituents `flowNarrow` re-admitted only because they are
 * subtypes of kept ones: in the false branch `reference` keeps exactly the
 * snapshot's constituents.
 */
export declare function flowExcluded<S>(
  snapshot: S,
): <R>(reference: R) => reference is FlowExcluded<R, S>;

/**
 * An unknown branch outcome: a generated function that only reports whether
 * its element renders guards with it (`if (!flowBranch) return null;`).
 */
export declare const flowBranch: boolean;

/** The single alias of a `v-for` frame over `source`, as Vue's `renderList` iterates it. */
export declare function flowEach1<V>(source: readonly V[] | null | undefined): V;
export declare function flowEach1<V>(source: Iterable<V> | null | undefined): V;
export declare function flowEach1(source: number | null | undefined): number;
export declare function flowEach1<S extends object>(source: S | null | undefined): S[keyof S];

/** The `[value, key]` aliases of a `v-for` frame over `source`. */
export declare function flowEach2<V>(source: readonly V[] | null | undefined): [V, number];
export declare function flowEach2<V>(source: Iterable<V> | null | undefined): [V, number];
export declare function flowEach2(source: number | null | undefined): [number, number];
export declare function flowEach2<S extends object>(
  source: S | null | undefined,
): [S[keyof S], keyof S];

/** The `[value, key, index]` aliases of a `v-for` frame over `source`. */
export declare function flowEach3<V>(source: readonly V[] | null | undefined): [V, number, number];
export declare function flowEach3<V>(source: Iterable<V> | null | undefined): [V, number, number];
export declare function flowEach3(source: number | null | undefined): [number, number, number];
export declare function flowEach3<S extends object>(
  source: S | null | undefined,
): [S[keyof S], keyof S, number];
