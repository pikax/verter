import Comp from "./components/JsChecked.vue";
import { count, identity } from "./js-checked-script.js";
import { title } from "./js-checked-template";

export type Instance = InstanceType<typeof Comp>;
export const instance: Instance = {} as Instance;
export const stp4HoverTarget: string = identity(count);
export const stp4DefinitionTarget: typeof Comp = Comp;
void instance;
void title;
