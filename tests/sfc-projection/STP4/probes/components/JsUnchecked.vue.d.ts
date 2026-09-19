import type { ComponentPublicInstance } from "vue";

type UncheckedProps = { readonly count?: number };

export declare class Comp {
  constructor(props?: UncheckedProps);
  readonly $props: UncheckedProps;
  readonly count: number;
}

export interface Comp extends ComponentPublicInstance<UncheckedProps> {}

export default Comp;
