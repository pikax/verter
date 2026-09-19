import Comp from "./components/Ratified.vue";

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = {} as Instance;

export const stp8DefinitionTarget: typeof Comp = Comp;

// STP8-inference-contract: T and U infer together from one whole-signature
// construction (`rows` fixes T, `project` consumes T and fixes U), so no
// contributing channel waits for a post-specialization pass.
export const coupled = new Comp({ rows: ["a", "b"], project: (row) => row.length });
export const coupledValue: number | undefined = coupled.value;
export const coupledOnChange = coupled.$props.onChange?.(coupled.value ?? 0);
export const coupledSlot = coupled.$slots.default?.({ row: "a", value: 1 });
export const coupledModel: unknown = coupled.$props.modelValue;

// Every U-dependent channel must carry the inferred `number`: a widened or
// collapsed U on the event payload, the model value, or the slot scope fails
// these exact-type witnesses.
type Exact<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;
export const coupledOnChangePayloadIsNumber: Exact<
  Parameters<NonNullable<typeof coupled.$props.onChange>>[0],
  number
> = true;
export const coupledModelValueIsNumber: Exact<
  Exclude<typeof coupled.$props.modelValue, undefined>,
  number
> = true;
export const coupledSlotScopeIsRowValue: Exact<
  Parameters<NonNullable<typeof coupled.$slots.default>>[0],
  { row: string; value: number }
> = true;
export const stp8HoverTarget: number = coupledValue === undefined ? 0 : 1;

// The one-binder construction stays a specialization of the same family:
// `rows` alone fixes T and U keeps its binder default.
export const inferred = new Comp({ rows: ["a", "b"] });
export const inferredValue: unknown = inferred.value;

export const explicit = new Comp<string, number>({
  rows: [],
  project: (row) => row.length,
});
export const explicitValue: number | undefined = explicit.value;
