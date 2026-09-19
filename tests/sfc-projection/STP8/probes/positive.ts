import Comp from "./components/Ratified.vue";

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = {} as Instance;

export const stp8HoverTarget: number = instance.first === undefined ? 0 : 1;
export const stp8DefinitionTarget: typeof Comp = Comp;

export const inferred = new Comp({ items: ["a", "b"] });
export const inferredFirst: string | undefined = inferred.first;

export const explicit = new Comp<number>({ items: [0] });
export const explicitFirst: number | undefined = explicit.first;

export const emitPayload = explicit.$emit("change", 0);
export const slotPayload = explicit.$slots.default?.({ item: 0 });
