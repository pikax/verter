import Comp from "./components/Coupled.vue";

/** Clean twin: independent attribute permutation keeps the same contextual types. */
export const permA = new Comp({
  rows: [{ id: 1, name: "a" }],
  project: (row) => row.name,
});
export const permB = new Comp({
  project: (row: { id: number; name: string }) => row.name,
  rows: [{ id: 1, name: "a" }],
});
export const permSame: typeof permA.value = permB.value;
export const stp3HoverTarget: string = permA.value;
export const stp3DefinitionTarget: typeof Comp = Comp;
