// Curated semantic oracle - `defineModel`.
//
// The intended Vue semantics of `defineModel<boolean>()`: a writable model ref whose
// `.value` is the declared model type, two-way bound to the parent's `v-model`. The
// unwrapped `.value` must be `boolean`, never the ref wrapper.
//
// Each anchor's query target is the LAST identifier on its line.

interface ModelRef<T> {
  value: T;
}

declare const open: ModelRef<boolean>;

const isOpen = open.value; // @dx-anchor model.value

export { isOpen };
