// A PLAIN `.ts` file (not a carrier) importing `.vue`/`.svelte` components.
// Under the project-bound external-TS contract the bare `.vue`/`.svelte`
// imports resolve to the component IDE carriers and the components' public
// surfaces flow into this module — the §2.9 enhanced-DX bar.
import Comp from "./Comp.vue";
import Widget from "./Widget.svelte";

// Reference the imported component values (definition / find-all-references /
// rename anchors that must reach the component source through the carrier).
export const comp = Comp;
export const widget = Widget;

export function useComponents(): [typeof Comp, typeof Widget] {
  return [comp, widget];
}
