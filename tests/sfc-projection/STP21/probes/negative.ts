import Emitter from "./components/Emitter.vue";

export type Instance = InstanceType<typeof Emitter>;

// STP21-static-handler: a static `onSave="handler"` spelling is text, not
// an implicit function-variable reference.
export const broken: Instance["$props"] = {
  onSaveItem: "handler",
};
