import { createApp, h, type Component } from "vue";
import Comp from "./components/Ratified.vue";

// STP8-abi-contamination: the inherited STP2-vue-utilities recipe exercised on
// the selected two-binder encoding. Every Vue public utility must observe the
// ratified constructor's props surface, not a checker-only projection of it.
// h() checks props against the binder defaults; TSX infers T (and U) from the
// attributes, so the twin observes both the surface and the specialization.
export const app = createApp(Comp, { rows: ["a"] });
export const vnode = h(Comp, { rows: [1], project: (row) => `${row}` });
export const element = <Comp rows={["x"]} project={(row: string) => row.toUpperCase()} />;
export const asComponent: Component = Comp;

void app;
void vnode;
void element;
void asComponent;
