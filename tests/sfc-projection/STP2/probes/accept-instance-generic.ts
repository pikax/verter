import Comp from "./components/Generic.vue";
import { ConstrainedFoo } from "./foo-control";

export type Unbound = InstanceType<typeof Comp>;

export const unbound: Unbound = {} as Unbound;

export const unboundValue: unknown = unbound.value;
export const unboundTest: unknown = unbound.$props.test;
export const unboundLabel: string = unbound.label;
export const unboundResetPresence: unknown = unbound.$emit;
export const stp2HoverTarget: unknown = unbound.value;
export const stp2DefinitionTarget: typeof Comp = Comp;

export const zeroArg = new Comp();
export const zeroArgValue: unknown = zeroArg.value;

export const constrained = new ConstrainedFoo({ test: 1 });
export const constrainedValue: number = constrained.value;
