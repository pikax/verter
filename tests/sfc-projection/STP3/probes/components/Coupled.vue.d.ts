import type { ComponentPublicInstance, Slot } from "vue";

type CoupledProps<T, U> = {
  readonly rows?: readonly T[];
  readonly project?: (row: T) => U;
  readonly modelValue?: U;
  readonly onChange?: (value: U) => void;
  readonly "onUpdate:modelValue"?: (value: U) => void;
};
type CoupledEmit<U> = ((event: "change", value: U) => void) &
  ((event: "update:modelValue", value: U) => void);
type CoupledSlots<T, U> = Readonly<{
  default?: Slot<{ row: T; value: U }>;
}>;
type CoupledExposed<T, U> = { readonly value: U };

export declare class Comp<T = unknown, U = unknown> {
  constructor(props?: CoupledProps<T, U>);
  readonly $props: CoupledProps<T, U>;
  readonly $emit: CoupledEmit<U>;
  readonly $slots: CoupledSlots<T, U>;
  readonly value: U;
}

export interface Comp<T = unknown, U = unknown> extends ComponentPublicInstance<
  CoupledProps<T, U>,
  CoupledExposed<T, U>,
  {},
  {},
  {},
  { change: [value: U]; "update:modelValue": [value: U] }
> {}

export default Comp;
