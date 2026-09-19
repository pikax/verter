import type { ComponentPublicInstance } from "vue";
import { bump, type SharedCount } from "../shared/logic";

type OwnerProps = { readonly count?: SharedCount };

export declare class Comp {
  constructor(props?: OwnerProps);
  readonly $props: OwnerProps;
  readonly bump: typeof bump;
}

export interface Comp extends ComponentPublicInstance<OwnerProps> {}

export default Comp;
