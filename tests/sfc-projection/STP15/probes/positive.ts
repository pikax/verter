import Comp, { countRef } from "./components/Counter.vue";

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = {} as Instance;

export const stp15DefinitionTarget: typeof Comp = Comp;

// STP15-read-ref: the template reads the top-level ref unwrapped while the
// script side keeps the `Ref` wrapper (`.value`).
export const stp15HoverTarget: number = instance.count;
export const scriptSide: number = countRef.value;

// STP15-setter-domain: the writable computed accepts its declared setter
// domain (`string`), not merely its read type (`number`).
export const readLabel: number = instance.label;
export function renameLabel(next: string): void {
  instance.setLabel(next);
}

// Model refs and plain members stay writable through the write target.
export function update(step: number): void {
  instance.count += step;
  instance.modelValue = instance.title || "untitled";
}

// InstanceType of the imported component preserves its public API.
export const api: Instance = instance;
