import Comp from "@stp6/lib/concrete";

export type Instance = InstanceType<typeof Comp>;
export const instance: Instance = {} as Instance;
export const stp6HoverTarget: () => void = instance.reset;
export const stp6DefinitionTarget: typeof Comp = Comp;
