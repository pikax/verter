import Comp from "./components/Concrete.vue";

export type Instance = InstanceType<typeof Comp>;
export const instance: Instance = {} as Instance;

/** Dirty: zero-payload $emit must reject a callback payload. */
export const wrong = instance.$emit("reset", 42);
export const stp2HoverTarget = wrong;
export const stp2DefinitionTarget: typeof Comp = Comp;
