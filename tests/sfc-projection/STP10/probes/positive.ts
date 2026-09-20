import Comp from "./components/Ratified.vue";

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = {} as Instance;

export const stp10DefinitionTarget: typeof Comp = Comp;

export const coupled = new Comp({ rows: ["a", "b"], project: (row) => row.length });
export const coupledValue: number | undefined = coupled.value;
export const coupledOnChange = coupled.$props.onChange?.(coupled.value ?? 0);
export const coupledSlot = coupled.$slots.default?.({ row: "a", value: 1 });
export const coupledModel: unknown = coupled.$props.modelValue;
export const stp10HoverTarget: number = coupledValue === undefined ? 0 : 1;

export const inferred = new Comp({ rows: ["a", "b"] });
export const inferredValue: unknown = inferred.value;

export const explicit = new Comp<string, number>({
  rows: [],
  project: (row) => row.length,
});
export const explicitValue: number | undefined = explicit.value;
