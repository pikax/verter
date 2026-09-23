import Comp from "./components/Counter.vue";

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = {} as Instance;

// STP15-readonly-write: a template assignment to a getter-only computed is
// a real type error, not a silent pass through a mutable alias.
export const broken = (instance.doubled = 5);
