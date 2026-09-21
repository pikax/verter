import type { ComponentPublicInstance } from "vue";

// Value-space dependencies captured through `typeof`: a unique symbol, a
// local class and a local function are each nameable in the emitted
// declaration rather than re-spelled as an anonymous structural type.
export declare const stp14Marker: unique symbol;

export declare class Tag {
  readonly id: number;
}

export declare function order(rows: readonly { id: number }[]): number;

// Lifted from `<script setup>` and re-parameterized over the component
// binder: the authored alias was free in the binder parameter, so the lifted
// declaration carries it instead of inlining the parameter's constraint.
type Selection<A extends { id: number }> = {
  readonly item: A;
  readonly key: A["id"];
};

// The second binder parameter's default refers to the first one, and keeps
// that binding after lifting.
type CaptureProps<A extends { id: number }, B extends readonly A[] = readonly A[]> = {
  readonly rows?: B;
  readonly selected?: Selection<A>;
  readonly tag?: typeof Tag;
  readonly marker?: typeof stp14Marker;
  readonly order?: typeof order;
};

export declare class Comp<
  A extends { id: number } = { id: number },
  B extends readonly A[] = readonly A[],
> {
  constructor(props?: CaptureProps<A, B>);
  readonly $props: CaptureProps<A, B>;
  readonly first: A | undefined;
}

export interface Comp<
  A extends { id: number } = { id: number },
  B extends readonly A[] = readonly A[],
> extends ComponentPublicInstance<CaptureProps<A, B>, { readonly first: A | undefined }> {}

export default Comp;
