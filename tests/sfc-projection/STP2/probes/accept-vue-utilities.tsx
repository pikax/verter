import { createApp, h, type Component } from "vue";
import Comp from "./components/Concrete.vue";

export const app = createApp(Comp, { msg: "ok" });
export const vnode = h(Comp, { msg: "ok" });
export const element = <Comp msg="ok" />;
export const asComponent: Component = Comp;
export const stp2HoverTarget: string = "ok";
export const stp2DefinitionTarget: typeof Comp = Comp;

void app;
void vnode;
void element;
void asComponent;
