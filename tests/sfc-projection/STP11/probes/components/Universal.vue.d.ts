import type { ComponentPublicInstance } from "vue";

type UniversalProps<T extends { id: number }> = {
  readonly rows?: readonly T[];
  readonly pick?: (row: T) => number;
};

// A generic component whose setup body awaits: the constructor and its
// instance are unchanged by the async checking wrapper.
export declare class Comp<T extends { id: number } = { id: number }> {
  constructor(props?: UniversalProps<T>);
  readonly $props: UniversalProps<T>;
  readonly first: T | undefined;
}

export interface Comp<T extends { id: number } = { id: number }> extends ComponentPublicInstance<
  UniversalProps<T>,
  { readonly first: T | undefined }
> {}

export default Comp;
