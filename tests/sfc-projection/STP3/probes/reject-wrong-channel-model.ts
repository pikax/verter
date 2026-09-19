import Comp from "./components/Coupled.vue";

/** Dirty: U=string from project; a number model write must be rejected. */
export const modelPin = new Comp({
  rows: [{ id: 1, name: "a" }],
  project: (row) => row.name,
});
export const wrongModel: typeof modelPin.$props.modelValue = 42;
export const stp3HoverTarget = wrongModel;
export const stp3DefinitionTarget: typeof Comp = Comp;
