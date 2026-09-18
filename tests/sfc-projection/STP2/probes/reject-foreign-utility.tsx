import { createApp, h } from "vue";
import Comp from "./components/Concrete.vue";

/** Dirty: foreign Vue utilities must reject a wrong callback/prop payload. */
export const wrongApp = createApp(Comp, { msg: 123 });
export const wrongH = h(Comp, { msg: 123 });
export const wrongTsx = <Comp msg={123} />;
export const stp2HoverTarget = wrongH;
export const stp2DefinitionTarget: typeof Comp = Comp;

void wrongApp;
void wrongTsx;
