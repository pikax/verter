// Constraint-negative probe for STP19-explicit: explicit arguments to the
// generated constructor are checked against the authored binder, and each
// violation is one TS2344 at the authored argument (every line carrying a
// violation is marked `// violates`).
import Picker from "./components/Picker.vue";

export const TextId = Picker<{ id: string }>; // violates
export type NumberInstance = InstanceType<typeof Picker<number>>; // violates
export const WrongField = Picker<{ id: number }, "name">; // violates
export const Accepted = Picker<{ id: number; name: string }, "name", [boolean]>;
