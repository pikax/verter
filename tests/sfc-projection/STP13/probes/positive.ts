import Comp from "./components/Options.vue";
import Merged, { storeKey } from "./components/Combined.vue";

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = {} as Instance;

export const stp13DefinitionTarget: typeof Comp = Comp;

// Options `this` members: data, computed, and methods stay typed.
export const title: string = instance.title;
export const stp13HoverTarget: number = instance.count;
export const doubled: number = instance.doubled;
export const label: string = instance.label;

export function bump(step: number): void {
  instance.increment(step);
}

// Mixin/extends-inherited members remain visible and correctly typed.
export const merged: InstanceType<typeof Merged> = {} as InstanceType<typeof Merged>;
export const fromMixin: number = merged.mixinCount;
export const key: string = storeKey;

// InstanceType of the imported Options component preserves its public API.
export const api: Instance = merged as unknown as Instance;
