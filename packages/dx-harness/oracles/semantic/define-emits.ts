// Curated semantic oracle - `defineEmits`.
//
// The intended Vue semantics of `defineEmits<{ submit: [value: string]; close: [] }>()`:
// the emit function is callable with each declared event name and its payload tuple
// (an overload per event). Hovering `emit` surfaces those overload signatures, so a
// codegen that dropped the `value: string` payload diverges from this oracle.
//
// Each anchor's query target is the LAST identifier on its line.

declare function emit(event: "submit", value: string): void;
declare function emit(event: "close"): void;

const trigger = emit; // @dx-anchor emit
trigger("submit", "ok");

export { trigger };
