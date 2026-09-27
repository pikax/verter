import Comp from "./components/Picker.vue";

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = {} as Instance;

// STP16-private-leak: a setup-private binding is not an instance member.
export const leaked = instance.secret;
