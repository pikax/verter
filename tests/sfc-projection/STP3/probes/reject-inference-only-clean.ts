import Comp from "./components/Coupled.vue";

/** Clean twin: callback stays in the inference transaction; T is number. */
export const callbackOnly = new Comp({
  project: (row: { id: number }) => String(row.id),
});
export const callbackOnlyRow = {} as Parameters<
  NonNullable<(typeof callbackOnly)["$slots"]["default"]>
>[0]["row"];
export const keptT: number = callbackOnlyRow.id;
export const stp3HoverTarget: number = keptT;
export const stp3DefinitionTarget: typeof Comp = Comp;
