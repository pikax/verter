import Comp from "./components/Coupled.vue";

/** Clean twin: sibling uses keep independent specializations. */
export const siblingA = new Comp({
  rows: ["alpha"],
  project: (row) => row,
});
export const siblingB = new Comp({
  rows: [1],
  project: (row) => row,
});
export const siblingAValue: string = siblingA.value;
export const siblingBValue: number = siblingB.value;
export const stp3HoverTarget: number = siblingBValue;
export const stp3DefinitionTarget: typeof Comp = Comp;
