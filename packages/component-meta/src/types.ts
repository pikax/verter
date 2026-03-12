/**
 * Component metadata types extracted from Vue SFCs.
 */

import type { TypeDescriptor } from "./type-ir.js";

export type ApiStyle = "composition" | "options" | "mixed";

export interface ComponentMeta {
  filePath: string;
  componentName: string;
  apiStyle: ApiStyle;
  props: PropMeta[];
  events: EventMeta[];
  slots: SlotMeta[];
  models: ModelMeta[];
  exposed: ExposedMeta[];
}

export interface PropMeta {
  name: string;
  type: TypeDescriptor;
  required: boolean;
  hasDefault: boolean;
  /** Original TS type annotation string (e.g. `"string | number"`). */
  rawType?: string;
  /** Vue runtime constructor names (e.g. `["String", "Number"]`). */
  runtimeTypes?: string[];
}

export interface EventMeta {
  name: string;
  payload: TypeDescriptor;
  hasValidator: boolean;
  isDeclared: boolean;
  /** Original emit signature string. */
  rawSignature?: string;
}

export interface SlotMeta {
  name: string;
  isScoped: boolean;
  bindings: SlotBinding[];
}

export interface SlotBinding {
  name: string;
  type: TypeDescriptor;
}

export interface ModelMeta {
  name: string;
  type: TypeDescriptor;
}

export interface ExposedMeta {
  name: string;
  type: TypeDescriptor;
}
