import type { ComponentPublicInstance } from "vue";

export type OptionsProps = {
  readonly title?: string;
  readonly initial?: number;
};

// Classic Options API component: props, data, computed (get and get/set),
// methods, and emits. The default export is constructor-shaped; instance
// members stay visible through `InstanceType<typeof Comp>`.
export declare class Comp {
  constructor(props?: OptionsProps);
  readonly $props: OptionsProps;
  readonly $emit: {
    (event: "change", value: number): void;
    (event: "reset"): void;
  };
  readonly title: string;
  count: number;
  readonly doubled: number;
  label: string;
  increment(step?: number): void;
  reset(): void;
}

export interface Comp extends ComponentPublicInstance<
  OptionsProps,
  { count: number },
  {},
  { doubled: number; label: string },
  { increment(step?: number): void; reset(): void },
  { change: [value: number]; reset: [] }
> {}

export default Comp;
