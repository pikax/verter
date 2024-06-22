import {
  MagicString,
  SFCParseResult,
  SFCScriptBlock,
  SFCTemplateBlock,
} from "@vue/compiler-sfc";
import type * as _babel_types from "@babel/types";
import { RootNode } from "@vue/compiler-core";
import { VerterSFCBlock } from "../utils/sfc";

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
  /**
   * This will only be resolved when running plugin
   */
  isAsync: boolean;
  generic: string | undefined;

  sfc: SFCParseResult;
  script: SFCScriptBlock | null;
  template: SFCTemplateBlock | null;

  s: MagicString;

  blocks: VerterSFCBlock[];

  // locations: LocationType[];
}

export type WalkResult = TypeLocation | undefined | void | Array<WalkResult>;

export interface PluginOption {
  name?: string;

  walk?: (
    state: _babel_types.Statement,
    context: ParseScriptContext
  ) => WalkResult;

  deepWalk?: (
    state: _babel_types.Statement,
    context: ParseScriptContext
  ) => WalkResult;

  /**
   * Processes the whole file
   * @param context
   * @returns
   */
  process?: (context: ParseScriptContext) => WalkResult;
}

export const enum LocationType {
  Props = "props",
  Emits = "emits",
  Slots = "slots",
  Options = "options",
  Model = "model",
  Expose = "expose",

  Template = "template",

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
  node?: _babel_types.Node | RootNode;
  // holds information if it was generated from script or scriptSetup,
  // this is necessary because when we walk the AST the babel nodes
  // are offset to the <script> and not to the file.
  isSetup?: boolean;

  content?: string;

  applyMap?(s: MagicString): void;
}

export interface TypeLocationDeclaration {
  type: LocationType.Declaration;
  node?:
    | _babel_types.VariableDeclaration
    | _babel_types.FunctionDeclaration
    | _babel_types.EnumDeclaration
    | _babel_types.ClassDeclaration;

  generated?: boolean;

  /**
   * 'global' adds before the generator function
   * 'pre': adds in the generator but before options
   * 'post': adds in generator but after options
   * 'end': adds in generator but at the end just before return
   * 'finish': adds outside of generator, to add exports, etc
   */
  context?: "global" | "pre" | "post" | "end" | "finish";

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
  generated?: boolean;
  node?: _babel_types.ImportDeclaration;
  items: ImportItem[];
  from: string;

  asType?: boolean;

  // block?: SFCBlock;
  offset?: number;
}

export interface TypeLocationExport {
  type: LocationType.Export;
  node: _babel_types.ImportDeclaration;
  offset?: number;

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
  varName?: string;
}
export interface TypeLocationProps {
  type: LocationType.Props;
  node: _babel_types.CallExpression | _babel_types.TSTypeLiteral;
  // always the `defineProps` expression
  expression: _babel_types.Expression;

  // parent of the expression, can be
  varName?: string;

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
      /**
       * The generic param index
       */
      index: number;
      /**
       * The generic name
       */
      name: string;
      /**
       * The generic constraint
       */
      constraint?: string;
      /**
       * The generic default value
       */
      default?: string;
      /**
       * The generic content contains name + constraint
       */
      content?: string;
    }
  ];
}

export interface TypeLocationTemplate {
  type: LocationType.Template;
  node: RootNode;

  content?: string;

  map?: any;
  // this causes compilation error
  // map?: ReturnType<MagicString["generateMap"]>;
}

export interface TypeLocationSlots {
  type: LocationType.Slots;
  node: _babel_types.CallExpression | _babel_types.TSTypeLiteral;
  // always the `defineProps` expression
  expression: _babel_types.Expression;

  // parent of the expression, can be
  varName?: string;

  content?: string;
  properties?: {
    node: _babel_types.Node;
    name: string;
    // aka type?
    content: string;
  }[];
}

export type TypeLocationMap = {
  [LocationType.Declaration]: TypeLocationDeclaration;
  [LocationType.Import]: TypeLocationImport;
  [LocationType.Emits]: TypeLocationEmits;
  [LocationType.Props]: TypeLocationProps;
  [LocationType.Slots]: TypeLocationSlots;
  [LocationType.Options]: BaseTypeLocation;
  [LocationType.Model]: BaseTypeLocation;
  [LocationType.Expose]: BaseTypeLocation;
  [LocationType.Export]: TypeLocationExport;
  [LocationType.Generic]: TypeLocationGeneric;
  [LocationType.Template]: TypeLocationTemplate;
};
export type ValueOf<T> = T[keyof T];

export type LocationByType = {
  [K in LocationType]?: K extends keyof TypeLocationMap
    ? Array<TypeLocationMap[K] & BaseTypeLocation>
    : never;
};

export type TypeLocation = BaseTypeLocation & ValueOf<TypeLocationMap>;
