import type { ComponentPublicInstance } from "vue";

type CheckedProps = { readonly count?: string };

export declare class Comp {
  constructor(props?: CheckedProps);
  readonly $props: CheckedProps;
  readonly count: string;
}

export interface Comp extends ComponentPublicInstance<CheckedProps> {}

export default Comp;
