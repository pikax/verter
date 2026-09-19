import Comp from "./components/Concrete.vue";

/** Clean twin: typed constructor rejects wrong props. */
export const wrong = new Comp({ notMsg: true });
export const stp2HoverTarget = wrong;
export const stp2DefinitionTarget: typeof Comp = Comp;
