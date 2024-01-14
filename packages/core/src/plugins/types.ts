import { SFCParseResult, SFCScriptBlock } from "@vue/compiler-sfc";
import type * as _babel_types from "@babel/types";

// TODO move somewhere else
export type VueAPISetup =
  | "defineProps"
  | "defineEmits"
  | "defineSlots"
  | "defineOptions"
  | "defineModel"
  | "defineExpose"
  | "withDefaults";

export interface ParseScriptContext {
  filename: string;
  id: string;

  isSetup: boolean;

  sfc: SFCParseResult;
  script: SFCScriptBlock | null;

  // locations: LocationType[];
}

export type WalkResult = TypeLocation | TypeLocation[] | undefined;

export interface PluginOption {
  name?: string;

  walk?: (
    state: _babel_types.Statement,
    context: ParseScriptContext
  ) => void | WalkResult;

  deepWalk?: (
    state: _babel_types.Statement,
    context: ParseScriptContext
  ) => void | WalkResult;

  process?: (
    context: ParseScriptContext,
    locations: TypeLocation[]
  ) => TypeLocation | TypeLocation[];
}

export const enum LocationType {
  Props = "props",
  Emits = "emits",
  Slots = "slots",
  Options = "options",
  Model = "model",
  Expose = "expose",

  Declaration = "declaration",
  Import = "import",
  export = "export",
}

export interface ImportItem {
  from: string;
  name: string;
  alias?: string;
  type?: boolean;
}

export interface BaseTypeLocation {
  type: LocationType;
  node: _babel_types.Node;

  content?: string;
}

export interface TypeLocationDeclaration {
  type: LocationType.Declaration;
  node: _babel_types.ObjectExpression;

  declaration: {
    /**
     * Sets the variable type
     * @default 'const'
     */
    type?: "const" | "var" | "let" | "type" | "interface" | "enum" | "function";
    /**
     * The variable name
     */
    name: string;
    /**
     * The variable content
     */
    content: string;
  };
}

export interface TypeLocationImport {
  type: LocationType.Import;
  node: _babel_types.ImportDeclaration;
  items: ImportItem[];
}

export interface TypeLocationEmits {
  type: LocationType.Emits;
  node: _babel_types.CallExpression;

  content?: string;
  properties?: {
    name: string;
    content: string;
  }[];
}
export interface TypeLocationProps {
  type: LocationType.Props;
  node: _babel_types.CallExpression;

  content?: string;
  properties?: {
    name: string;
    content: string;
  }[];
}

export type TypeLocation =
  | BaseTypeLocation
  | TypeLocationImport
  | TypeLocationEmits
  | TypeLocationProps
  | TypeLocationDeclaration;
