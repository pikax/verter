// Curated semantic oracle - auto-import shape.
//
// The intended Vue semantics of an auto-imported composable (`ref`): the symbol is
// available with the call shape `<T>(value: T) => Ref<T>` - as though imported from
// `vue` - and unwraps to a writable `.value` of the inferred element type.
//
// Each anchor's query target is the LAST identifier on its line.

interface Ref<T> {
  value: T;
}

declare function ref<T>(value: T): Ref<T>;

const counter = ref(0); // @dx-anchor autoImport.ref
const current = counter.value; // @dx-anchor autoImport.value

export { counter, current };
