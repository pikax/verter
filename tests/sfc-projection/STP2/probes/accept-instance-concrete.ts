import { ref, type Component, type ComponentPublicInstance } from "vue";
import Comp from "./components/Concrete.vue";

export type Instance = InstanceType<typeof Comp>;

export const instance: Instance = {} as Instance;
export const instanceRef = ref<InstanceType<typeof Comp> | null>(null);

export function reset(): void {
  instance.reset();
  instanceRef.value?.reset();
}

export const stp2HoverTarget: () => void = instance.reset;
export const stp2PropsMsg: string = instance.$props.msg;
export const stp2DefinitionTarget: typeof Comp = Comp;

const asComponent: Component = Comp;
void asComponent;
void instance.$emit;
void instance.$slots;
void instance.$props;
type CpiKeys = keyof ComponentPublicInstance;
export const cpiProps: CpiKeys = "$props";
export const cpiEmit: CpiKeys = "$emit";
export const cpiSlots: CpiKeys = "$slots";
