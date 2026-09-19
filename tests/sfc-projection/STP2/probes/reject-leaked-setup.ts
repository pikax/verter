import Comp from "./components/Concrete.vue";

export type Instance = InstanceType<typeof Comp>;
export const instance: Instance = {} as Instance;

/** Dirty: non-exposed setup binding must be absent from the instance type. */
export const leaked = instance.hidden;
export const stp2HoverTarget = leaked;
export const stp2DefinitionTarget: typeof Comp = Comp;
