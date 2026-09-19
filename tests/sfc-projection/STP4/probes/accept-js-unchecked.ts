import Comp from "./components/JsUnchecked.vue";
import { count } from "./js-unchecked-script.js";

export type Instance = InstanceType<typeof Comp>;
export const instance: Instance = {} as Instance;
export const stp4HoverTarget: string = count;
export const stp4DefinitionTarget: typeof Comp = Comp;
void instance;
