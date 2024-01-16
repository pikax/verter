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
  generic?: string | true | undefined;

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

  process?: (context: ParseScriptContext) => void | WalkResult;
}

export const enum LocationType {
  Props = "props",
  Emits = "emits",
  Slots = "slots",
  Options = "options",
  Model = "model",
  Expose = "expose",

  Generic = "generic",

  Declaration = "declaration",
  Import = "import",
  Export = "export",
}

export interface ImportItem {
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

  generated?: boolean;
  declaration: {
    /**
     * Sets the variable type
     * @default 'const'
     */
    type?: "const" | "var" | "let" | "type" | "interface" | "enum" | "function";
    /**
     * The variable name
     */
    name?: string;
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
  from: string;
}

export interface TypeLocationExport {
  type: LocationType.Export;
  node: _babel_types.ImportDeclaration;

  item: ImportItem & { default?: boolean; content?: string };
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

export interface TypeLocationGeneric {
  type: LocationType.Generic;
  node: _babel_types.TSTypeParameterDeclaration;

  items: [
    {
      name: string;
      content: string;
      index: number;
    }
  ];
}

export type TypeLocationMap = {
  [LocationType.Declaration]: TypeLocationDeclaration;
  [LocationType.Import]: TypeLocationImport;
  [LocationType.Emits]: TypeLocationEmits;
  [LocationType.Props]: TypeLocationProps;
  [LocationType.Slots]: BaseTypeLocation;
  [LocationType.Options]: BaseTypeLocation;
  [LocationType.Model]: BaseTypeLocation;
  [LocationType.Expose]: BaseTypeLocation;
  [LocationType.Export]: TypeLocationExport;
  [LocationType.Generic]: TypeLocationGeneric;
};
export type ValueOf<T> = T[keyof T];

export type LocationByType = {
  [K in LocationType]?: K extends keyof TypeLocationMap
    ? Array<TypeLocationMap[K]>
    : never;
};

export type TypeLocation = BaseTypeLocation & ValueOf<TypeLocationMap>;
