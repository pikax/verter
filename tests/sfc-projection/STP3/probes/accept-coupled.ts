import Comp from "./components/Coupled.vue";
import { ConstComp, DefaultComp, DepComp, Parent, RankComp, VariadicComp } from "./binders";

export const coupled = new Comp({
  rows: [{ id: 1, name: "a" }],
  project: (row) => row.name,
});

export const coupledValue: string = coupled.value;
coupled.$emit("change", "ok");
coupled.$emit("update:modelValue", "ok");

export const coupledSlot = {} as Parameters<NonNullable<(typeof coupled)["$slots"]["default"]>>[0];
export const coupledSlotRowId: number = coupledSlot.row.id;
export const coupledSlotValue: string = coupledSlot.value;
export const coupledModel: string | undefined = coupled.$props.modelValue;

export const callbackOnly = new Comp({
  project: (row: { id: number }) => String(row.id),
});
export const callbackOnlyValue: string = callbackOnly.value;
export const callbackOnlyRow = {} as Parameters<
  NonNullable<(typeof callbackOnly)["$slots"]["default"]>
>[0]["row"];
export const callbackOnlyRowId: number = callbackOnlyRow.id;

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

export const defaulted: string = new DefaultComp().value;
export const constLit = new ConstComp({ rows: ["a", "b"] as const, project: (row) => row });
export const constValue: "a" | "b" = constLit.value;
export const variadic = new VariadicComp({
  items: [1, "x"],
  project: (n, s) => `${n}${s}`,
});
export const variadicValue: string = variadic.value;
export const dep = new DepComp({ row: { id: 1, name: "a" }, key: "id" });
export const depSelected: number = dep.selected;
export const rank = new RankComp({
  rows: [{ id: 1 }],
  project: (row) => String(row.id),
});
export const rankSlot = {} as Parameters<NonNullable<(typeof rank)["$slots"]["default"]>>[0];
export const rankMapped: boolean = rankSlot.each((row) => row.id > 0);
export const parented = Parent({
  value: { id: 1 },
  child: { rows: [{ id: 1 }], project: (row) => String(row.id) },
});
export const parentedValue: string = parented.value;

export const stp3HoverTarget: string = coupled.value;
export const stp3DefinitionTarget: typeof Comp = Comp;
