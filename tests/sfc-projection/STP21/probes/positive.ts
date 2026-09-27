import Emitter from "./components/Emitter.vue";

export type Instance = InstanceType<typeof Emitter>;
export const instance: Instance = {} as Instance;

// STP21-event-alias: `save-item` and `saveItem` target one raw listener key.
const save = (value: number): void => void value;
export const aliases: Instance["$props"] = { onSaveItem: save };

// STP21-model-key: model event identity is retained behind its raw key.
export const model: Instance["$props"] = { "onUpdate:modelValue": (value) => void value };

// STP21-modifier: event-option suffixes do not erase the payload contract.
export const modified: Instance["$props"] = {
  onSaveOnceCapturePassive: (value) => void value,
};

// STP21-dynamic-name: each finite dynamic candidate must satisfy the
// component's declared listener contract. A primitive or the other
// candidate's payload is not assignable.
declare const dynamic: "save-item" | "cancel";
type DynamicListener<Name extends typeof dynamic> = Name extends "save-item"
  ? NonNullable<Instance["$props"]["onSaveItem"]>
  : Name extends "cancel"
    ? NonNullable<Instance["$props"]["onCancel"]>
    : never;
export const dynamicHandlers: {
  [Name in typeof dynamic]: DynamicListener<Name>;
} = {
  "save-item": save,
  cancel: (value: boolean): void => void value,
};

export const stp21HoverTarget: number = instance.count;
export const stp21DefinitionTarget: typeof Emitter = Emitter;
