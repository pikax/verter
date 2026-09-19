import type { ComponentPublicInstance, Slot } from "vue";

type RatifiedProps<T, U> = {
  readonly rows?: readonly T[];
  readonly project?: (row: T) => U;
  readonly modelValue?: U;
  readonly onChange?: (value: U) => void;
  readonly "onUpdate:modelValue"?: (value: U) => void;
};
type RatifiedEmit<U> = (event: "update:modelValue", value: U) => void;
type RatifiedSlots<T, U> = Readonly<{ default?: Slot<{ row: T; value: U }> }>;
type RatifiedExposed<U> = { readonly value: U | undefined };

export declare class Comp<T = unknown, U = unknown> {
  constructor(props?: RatifiedProps<T, U>);
  readonly $props: RatifiedProps<T, U>;
  readonly $emit: RatifiedEmit<U>;
  readonly $slots: RatifiedSlots<T, U>;
  readonly value: U | undefined;
}

export interface Comp<T = unknown, U = unknown> extends ComponentPublicInstance<
  RatifiedProps<T, U>,
  RatifiedExposed<U>,
  {},
  {},
  {},
  { "update:modelValue": [value: U] }
> {}

export default Comp;
