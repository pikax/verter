/**
 * Component metadata types extracted from Vue SFCs.
 */

import type { TypeDescriptor } from "./type-ir.js";

/** The API style detected in the component's script block. */
export type ApiStyle = "composition" | "options" | "mixed";

/** Structured metadata extracted from a Vue Single File Component. */
export interface ComponentMeta {
  /** File path or canonical ID of the source SFC. */
  filePath: string;
  /** Component name derived from the file name (e.g. `"MyButton"`). */
  componentName: string;
  /** Detected API style: Composition API, Options API, or both. */
  apiStyle: ApiStyle;
  /** Props declared via `defineProps` or Options API `props`. */
  props: PropMeta[];
  /** Events declared via `defineEmits` or Options API `emits`. */
  events: EventMeta[];
  /** Slots discovered in the template. */
  slots: SlotMeta[];
  /** Models declared via `defineModel`. */
  models: ModelMeta[];
  /** Members exposed via `defineExpose` or Options API `expose`. */
  exposed: ExposedMeta[];
}

/** Metadata for a single component prop. */
export interface PropMeta {
  /** Prop name as declared in `defineProps` or Options API. */
  name: string;
  /** Parsed type descriptor. */
  type: TypeDescriptor;
  /** Whether the prop is required (no default, no `?`). */
  required: boolean;
  /** Whether the prop has a default value (via `withDefaults` or Options API). */
  hasDefault: boolean;
  /** Original TS type annotation string (e.g. `"string | number"`). */
  rawType?: string;
  /** Vue runtime constructor names (e.g. `["String", "Number"]`). */
  runtimeTypes?: string[];
}

/** Metadata for a single component event. */
export interface EventMeta {
  /** Event name (e.g. `"click"`, `"update:modelValue"`). */
  name: string;
  /** Payload type descriptor. */
  payload: TypeDescriptor;
  /** Whether the event has a runtime validator function. */
  hasValidator: boolean;
  /** Whether the event is explicitly declared (vs. inferred from template usage). */
  isDeclared: boolean;
  /** Original emit signature string. */
  rawSignature?: string;
}

/** Metadata for a single template slot. */
export interface SlotMeta {
  /** Slot name (`"default"` for the unnamed slot). */
  name: string;
  /** Whether the slot exposes scoped bindings. */
  isScoped: boolean;
  /** Scoped slot bindings (empty for non-scoped slots). */
  bindings: SlotBinding[];
}

/** A single binding exposed by a scoped slot. */
export interface SlotBinding {
  /** Binding name available in the slot scope. */
  name: string;
  /** Type descriptor for the binding value. */
  type: TypeDescriptor;
}

/** Metadata for a `defineModel` declaration. */
export interface ModelMeta {
  /** Model name (`"modelValue"` for the default model). */
  name: string;
  /** Type descriptor for the model value. */
  type: TypeDescriptor;
}

/** Metadata for a member exposed via `defineExpose`. */
export interface ExposedMeta {
  /** Exposed member name. */
  name: string;
  /** Type descriptor for the exposed value. */
  type: TypeDescriptor;
}
