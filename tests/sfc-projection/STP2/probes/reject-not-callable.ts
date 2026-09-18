import Comp from "./components/Concrete.vue";

/** Dirty: consuming the generated default export as a call. Must be a type error. */
export const called = Comp({ msg: "no" });
export const stp2HoverTarget = called;
export const stp2DefinitionTarget: typeof Comp = Comp;
