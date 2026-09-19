import Comp from "./components/Coupled.vue";

/** Default / const / variadic / dependent binders, higher-rank slots, parent forwarding. */

export declare class DefaultComp<T = string, U = T> {
  constructor(props?: { rows?: T[]; project?: (row: T) => U });
  readonly value: U;
}

export declare class ConstComp<const T, U = T> {
  constructor(props?: { rows?: readonly T[]; project?: (row: T) => U });
  readonly value: U;
}

export declare class VariadicComp<T extends unknown[], U> {
  constructor(props?: { items?: [...T]; project?: (...items: T) => U });
  readonly value: U;
}

export declare class DepComp<T, K extends keyof T = keyof T> {
  constructor(props?: { row?: T; key?: K });
  readonly selected: T[K];
}

export declare class RankComp<T, U> {
  constructor(props?: { rows?: T[]; project?: (row: T) => U });
  readonly $slots: {
    default?: (props: { row: T; value: U; each: <R>(fn: (row: T) => R) => R }) => unknown;
  };
}

export function Parent<P>(props: {
  value: P;
  child: ConstructorParameters<typeof Comp<P, string>>[0];
}): InstanceType<typeof Comp<P, string>> {
  return new Comp(props.child);
}
