import type { ComponentPublicInstance, Slot } from "vue";

type ConcreteProps = { readonly msg: string };
type ConcreteEmit = (event: "reset", ...args: []) => void;
type ConcreteSlots = Readonly<{
  default?: Slot<{ msg: string }>;
}>;
type ConcreteExposed = { reset(): void };

export declare class Comp {
  constructor(props?: ConcreteProps);
  readonly $props: ConcreteProps;
  readonly $emit: ConcreteEmit;
  readonly $slots: ConcreteSlots;
  reset(): void;
}

export interface Comp extends ComponentPublicInstance<
  ConcreteProps,
  ConcreteExposed,
  {},
  {},
  {},
  { reset: [] }
> {}

export default Comp;
