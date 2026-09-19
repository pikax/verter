import type { ComponentPublicInstance, ShallowUnwrapRef, Slot } from "vue";
import type { Row } from "./row";

export type { Row };

type GenericProps<T, U> = {
  readonly rows?: readonly T[];
  readonly project?: (row: T) => U;
  readonly modelValue?: U;
  readonly onChange?: (value: U) => void;
  readonly "onUpdate:modelValue"?: (value: U) => void;
};
type GenericEmit<U> = ((event: "change", value: U) => void) &
  ((event: "update:modelValue", value: U) => void);
type GenericSlots<T, U> = Readonly<{
  default?: Slot<{ row: T; value: U }>;
}>;
type GenericExposed<T, U> = { readonly value: U };

export declare class Comp<T = unknown, U = unknown> {
  constructor(props?: GenericProps<T, U>);
  readonly $props: GenericProps<T, U>;
  readonly $emit: GenericEmit<U>;
  readonly $slots: GenericSlots<T, U>;
  // The exposed `value` is read through the public instance, which unwraps
  // refs; the declared member must carry that same unwrapped type.
  readonly value: ShallowUnwrapRef<GenericExposed<T, U>>["value"];
}

export interface Comp<T = unknown, U = unknown> extends ComponentPublicInstance<
  GenericProps<T, U>,
  GenericExposed<T, U>,
  {},
  {},
  {},
  { change: [value: U]; "update:modelValue": [value: U] }
> {}

export default Comp;
