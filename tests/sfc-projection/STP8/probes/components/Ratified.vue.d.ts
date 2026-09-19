import type { ComponentPublicInstance, Slot } from "vue";

type RatifiedProps<T> = { readonly items?: readonly T[]; readonly label?: string };
type RatifiedEmit<T> = (event: "change", ...args: [value: T]) => void;
type RatifiedSlots<T> = Readonly<{ default?: Slot<{ item: T }> }>;
type RatifiedExposed<T> = { readonly first: T | undefined; reset(): void };

export declare class Comp<T = unknown> {
  constructor(props?: RatifiedProps<T>);
  readonly $props: RatifiedProps<T>;
  readonly $emit: RatifiedEmit<T>;
  readonly $slots: RatifiedSlots<T>;
  readonly first: T | undefined;
  reset(): void;
}

export interface Comp<T = unknown> extends ComponentPublicInstance<
  RatifiedProps<T>,
  RatifiedExposed<T>,
  {},
  {},
  {},
  { change: [value: T] }
> {}

export default Comp;
