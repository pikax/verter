import Comp from "./components/Coupled.vue";

/** Dirty: U=string from project; event payload.toFixed must be rejected. */
export const wrong = new Comp({
  rows: [{ id: 1, name: "a" }],
  project: (row) => row.name,
  onChange: (value) => value.toFixed(2),
});
export const stp3HoverTarget = wrong;
export const stp3DefinitionTarget: typeof Comp = Comp;
