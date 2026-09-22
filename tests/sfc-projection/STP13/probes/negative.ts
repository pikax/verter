import Comp from "./components/Options.vue";

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = {} as Instance;

// A misspelled Options member is a real type error, not a silent `any`.
export const broken = instance.cout;
