type __BenchInit = 0;
declare const RawSymbol: unique symbol;
interface Ref<T> {
  value: T;
  ref: true;
}
type UnwrapRef<T> = T extends Ref<infer V> ? UnwrapRefSimple<V> : UnwrapRefSimple<T>;
type UnwrapRefSimple<T> = T extends
  | string
  | number
  | boolean
  | Ref<unknown>
  | { [RawSymbol]?: true }
  ? T
  : T extends object
    ? { [K in keyof T]: UnwrapRef<T[K]> }
    : T;
type N0 = { o0: Ref<number>; v0: Ref<boolean> };
type N1 = { o1: N0; v1: Ref<string> };
type N2 = { o2: N1; v2: Ref<boolean> };
type N3 = { o3: N2; v3: Ref<string> };
type N4 = { o4: N3; v4: Ref<boolean> };
type NestedProbe = UnwrapRef<N4>;
export {};
