import Comp from "./components/Generic.vue";
import { Foo } from "./foo-control";

export type NumberInstance = InstanceType<typeof Comp<number>>;
export type FooNumber = InstanceType<typeof Foo<number>>;

export const NumberComp = Comp<number>;
export const numberInstance: NumberInstance = {} as NumberInstance;
export const fooNumber: FooNumber = {} as FooNumber;

export const selectedTest: number = numberInstance.$props.test;
export const selectedValue: number = numberInstance.value;
export const selectedChangePayload: number = {} as Parameters<NumberInstance["$emit"]>[1];
export const selectedSlotValue: number = {} as Parameters<
  NonNullable<NumberInstance["$slots"]["default"]>
>[0]["value"];
export const fooValue: number = fooNumber.value;
export const stp2HoverTarget: number = numberInstance.value;
export const stp2DefinitionTarget: typeof Comp = Comp;

numberInstance.$emit("change", selectedChangePayload);
