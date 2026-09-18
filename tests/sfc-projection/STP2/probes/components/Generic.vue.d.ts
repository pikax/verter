import type { ComponentPublicInstance, Slot } from "vue";

type GenericProps<T> = { readonly test: T };
type GenericEmit<T> = ((event: "change", value: T) => void) &
  ((event: "change", ...args: unknown[]) => void);
type GenericSlots<T> = Readonly<{
  default?: Slot<{ value: T }>;
}>;
type GenericExposed<T> = { readonly value: T };

export declare class Comp<T = unknown> {
  constructor(props?: GenericProps<T>);
  readonly $props: GenericProps<T>;
  readonly $emit: GenericEmit<T>;
  readonly $slots: GenericSlots<T>;
  readonly value: T;
  readonly label: string;
}

export interface Comp<T = unknown> extends ComponentPublicInstance<
  GenericProps<T>,
  GenericExposed<T>,
  {},
  {},
  {}
> {}

export default Comp;
