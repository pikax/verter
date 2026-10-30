// A reconstruction of a Pinia options store, not a copy of an original
// benchmark scenario. The declarations restate Pinia 3.0.4's options-store
// types (`defineStore`'s options overload and the store types it returns)
// and Vue 3.5's `UnwrapRef`, trimmed to the arms an options store reads.
// The global `ThisType` marker is not declared here: each test supplies it
// (from a library, from an authored `declare global`, or not at all).
type Exclude<T, U> = T extends U ? never : T;
type Pick<T, K extends keyof T> = { [P in K]: T[P] };
type Omit<T, K extends keyof any> = Pick<T, Exclude<keyof T, K>>;
type Record<K extends keyof any, T> = { [P in K]: T };

declare const RefSymbol: unique symbol;
declare const RawSymbol: unique symbol;
declare const ShallowRefMarker: unique symbol;
declare const ShallowReactiveMarker: unique symbol;
interface Ref<T = any, S = T> {
  get value(): T;
  set value(_: S);
  [RefSymbol]: true;
}
type ShallowRef<T = any, S = T> = Ref<T, S> & { [ShallowRefMarker]?: true };
interface RefUnwrapBailTypes {}
type ShallowReactiveBrand = { [ShallowReactiveMarker]: never };
type Primitive = string | number | boolean | bigint | symbol | undefined | null;
type Builtin = Primitive | Function | RegExp;
type UnwrapRef<T> =
  T extends ShallowRef<infer V, unknown>
    ? V
    : T extends Ref<infer V, unknown>
      ? UnwrapRefSimple<V>
      : UnwrapRefSimple<T>;
type UnwrapRefSimple<T> = T extends
  | Builtin
  | Ref
  | RefUnwrapBailTypes[keyof RefUnwrapBailTypes]
  | { [RawSymbol]?: true }
  ? T
  : T extends ShallowReactiveBrand
    ? T
    : T extends Map<infer K, infer V>
      ? Map<K, UnwrapRefSimple<V>> & UnwrapRef<Omit<T, keyof Map<any, any>>>
      : T extends Set<infer V>
        ? Set<UnwrapRefSimple<V>> & UnwrapRef<Omit<T, keyof Set<any>>>
        : T extends ReadonlyArray<any>
          ? { [K in keyof T]: UnwrapRefSimple<T[K]> }
          : T extends object
            ? { [P in keyof T]: P extends symbol ? T[P] : UnwrapRef<T[P]> }
            : T;

type StateTree = Record<PropertyKey, any>;
type _Method = (...args: any[]) => any;
type _ActionsTree = Record<string, _Method>;
type _GettersTree<S extends StateTree> = Record<
  string,
  ((state: UnwrapRef<S> & UnwrapRef<PiniaCustomStateProperties<S>>) => any) | (() => any)
>;
interface PiniaCustomStateProperties<S extends StateTree = StateTree> {}
interface PiniaCustomProperties<
  Id extends string = string,
  S extends StateTree = StateTree,
  G = _GettersTree<S>,
  A = _ActionsTree,
> {}
declare const ComputedRefSymbol: unique symbol;
declare const WritableComputedRefSymbol: unique symbol;
interface BaseComputedRef<T, S = T> extends Ref<T, S> {
  [ComputedRefSymbol]: true;
}
interface ComputedRef<T = any> extends BaseComputedRef<T> {
  readonly value: T;
}
interface WritableComputedRef<T, S = T> extends BaseComputedRef<T, S> {
  [WritableComputedRefSymbol]: true;
}
type Readonly<T> = { readonly [P in keyof T]: T[P] };
type _StoreWithGetters<G> = _StoreWithGetters_Readonly<G> & _StoreWithGetters_Writable<G>;
type _StoreWithGetters_Readonly<G> = {
  readonly [K in keyof G as G[K] extends (...args: any[]) => any
    ? K
    : ComputedRef extends G[K]
      ? K
      : never]: G[K] extends (...args: any[]) => infer R ? R : UnwrapRef<G[K]>;
};
type _StoreWithGetters_Writable<G> = {
  [K in keyof G as G[K] extends WritableComputedRef<any> ? K : never]: G[K] extends Readonly<
    WritableComputedRef<infer R>
  >
    ? R
    : never;
};
interface StoreProperties<Id extends string> {
  $id: Id;
}
interface _StoreWithState<
  Id extends string,
  S extends StateTree,
  G,
  A,
> extends StoreProperties<Id> {
  $state: UnwrapRef<S> & PiniaCustomStateProperties<S>;
  $reset(): void;
}
type Store<Id extends string = string, S extends StateTree = {}, G = {}, A = {}> = _StoreWithState<
  Id,
  S,
  G,
  A
> &
  UnwrapRef<S> &
  _StoreWithGetters<G> &
  (_ActionsTree extends A ? {} : A) &
  PiniaCustomProperties<Id, S, G, A> &
  PiniaCustomStateProperties<S>;
interface DefineStoreOptionsBase<S extends StateTree, Store> {}
interface DefineStoreOptions<
  Id extends string,
  S extends StateTree,
  G,
  A,
> extends DefineStoreOptionsBase<S, Store<Id, S, G, A>> {
  id: Id;
  state?: () => S;
  getters?: G &
    ThisType<UnwrapRef<S> & _StoreWithGetters<G> & PiniaCustomProperties> &
    _GettersTree<S>;
  actions?: A &
    ThisType<
      A & UnwrapRef<S> & _StoreWithState<Id, S, G, A> & _StoreWithGetters<G> & PiniaCustomProperties
    >;
}
interface StoreDefinition<
  Id extends string = string,
  S extends StateTree = StateTree,
  G = _GettersTree<S>,
  A = _ActionsTree,
> {
  (): Store<Id, S, G, A>;
  $id: Id;
}
declare function defineStore<
  Id extends string,
  S extends StateTree = {},
  G extends _GettersTree<S> = {},
  A = {},
>(id: Id, options: Omit<DefineStoreOptions<Id, S, G, A>, "id">): StoreDefinition<Id, S, G, A>;

export const useCounterStore = defineStore("counter", {
  state: () => ({ count: 0, items: [] as Array<{ id: number }>, name: "counter" }),
  getters: {
    double: (state) => state.count * 2,
    first: (state) => state.items[0],
  },
  actions: {
    increment() {
      this.count++;
      return this.count;
    },
    add(id: number) {
      const item = { id };
      this.items.push(item);
      return item;
    },
  },
});
type St = ReturnType<typeof useCounterStore>;
