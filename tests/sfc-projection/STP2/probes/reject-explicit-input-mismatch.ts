import Comp from "./components/Generic.vue";

export const NumberComp = Comp<number>;

/** Dirty: explicit number specialization must reject a string prop. */
export const wrongNew = new Comp<number>({ test: "wrong" });
export const wrongAlias = new NumberComp({ test: "wrong" });
export const stp2HoverTarget = wrongNew;
export const stp2DefinitionTarget: typeof Comp = Comp;
