import { ref } from "vue";
import Comp from "./components/Picker.vue";

type Equals<A, B> =
  (<X>() => X extends A ? 1 : 2) extends <X>() => X extends B ? 1 : 2 ? true : false;

// STP16-required-api: the ordinary import stays constructor-shaped and its
// instance type drives a template ref.
export type Instance = InstanceType<typeof Comp>;
export const stp16DefinitionTarget: typeof Comp = Comp;
export const pickerRef = ref<Instance | null>(null);
export const constructorParameters: Equals<
  ConstructorParameters<typeof Comp>,
  [props: Instance["$props"]]
> = true;
export const baseMember = (instance: Instance): Element | null => instance.$el;
// A required prop is required at the constructor.
// @ts-expect-error `test` is required
export const missingRequired = new Comp({});

// STP16-public-specialization: an explicit `Comp<number>` and a props-inferred
// `new Comp({ test: 0 })` both specialize every generic-dependent member.
export const NumberPicker = Comp<number>;
export type NumberInstance = InstanceType<typeof Comp<number>>;
export const explicitNumber: NumberInstance = new NumberPicker({ test: 1 });
export const stp16HoverTarget = explicitNumber.$props.test;
export const inferred = new Comp({ test: 0 });
export const inferredProp: Equals<typeof inferred.$props.test, 0> = true;
export const specializedExpose: Equals<NumberInstance["current"], number | undefined> = true;
export const specializedSlot: Equals<
  Parameters<NumberInstance["$slots"]["default"]>[0]["item"],
  number
> = true;
export function emitSpecialized(instance: NumberInstance): void {
  instance.$emit("change", 3);
  instance.$emit("close");
  instance.$emit("update:open", true);
  // @ts-expect-error the event payload follows the selected `number`
  instance.$emit("change", "3");
}
export const listener: Equals<
  NumberInstance["$props"]["onChange"],
  ((value: number) => any) | undefined
> = true;

// STP16-public-members: exposed members keep their exact types.
export function exposed(instance: NumberInstance): void {
  instance.reset();
}

// STP16-type-precision: non-generic fields keep their authored types; the
// unbound instance keeps TypeScript's constraint precision, never `never`.
export const labelPrecision: Equals<Instance["$props"]["label"], string | undefined> = true;
export const modelPrecision: Equals<Instance["$props"]["open"], boolean | undefined> = true;
export const unboundPrecision: Equals<Instance["$props"]["test"], string | number> = true;
export const staticName: Equals<typeof Comp.name, "Picker"> = true;

// STP16-generic-constraint: an explicit argument outside the constraint is a
// real diagnostic, not an overload fallback.
// @ts-expect-error `boolean` does not satisfy `string | number`
export type Invalid = InstanceType<typeof Comp<boolean>>;

// STP16-public-callable: the default export is not callable.
// @ts-expect-error a Vue component constructor has no call signature
Comp({ test: 0 });

// STP16-private-leak: setup-private bindings stay off the instance.
// @ts-expect-error `secret` is not exposed
export const privateMember = explicitNumber.secret;
