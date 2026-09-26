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

// STP21-dynamic-name: each finite dynamic candidate keeps its own contract.
declare const dynamic: "save-item" | "cancel";
export const dynamicHandlers: Record<typeof dynamic, unknown> = {
  "save-item": save,
  cancel: (value: boolean): void => void value,
};

export const stp21HoverTarget: number = instance.count;
export const stp21DefinitionTarget: typeof Emitter = Emitter;
