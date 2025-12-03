import { MagicString } from "@vue/compiler-sfc";
import { ParsedBlock } from "../parser/types";
import { ObjectExpression, VerterASTNode, IdentifierName } from "../parser/ast";
import { Node, ExpressionNode } from "@vue/compiler-core";
import { GenericInfo } from "../parser/script/generic";
import { ScriptItem } from "../parser/script";

export type ProcessContext = {
  filename: string;
  s: MagicString;
  /**
   * Uses the passed `s` instead of clone it
   */
  override?: boolean;
  blockNameResolver: (name: string) => string;

  isAsync: boolean;
  generic: GenericInfo | null;
  isTS: boolean;

  /**
   * Current block
   */
  block: ParsedBlock;
  blocks: ParsedBlock[];

  items: ProcessItem[];
};

export type ProcessPlugin<T, C extends ProcessContext> = {
  name?: string;

  enforce?: "pre" | "post";

  /**
   * Transforms all the items
   * @param item
   * @param context
   * @returns
   */
  transform?: (item: T, s: MagicString, context: C) => void;

  post?: (s: MagicString, context: C) => void;
  pre?: (s: MagicString, context: C) => void;
};

export const enum ProcessItemType {
  Import = "import",
  Warning = "warning",
  Error = "error",

  /**
   * Contains the bindings for the component
   */
  Binding = "binding",
  /**
   * Contains macro information
   */
  MacroBinding = "macro-binding",
  /**
   * Contains Macro return information
   */
  MacroReturn = "macro-return",
  /**
   * Contains the component options, for example `defineOptions`
   */
  Options = "options",

  /**
   * Used to describe the defineModel variables
   */
  DefineModel = "define-model",

  InheritAttrs = "inherit-attrs",
}
export type ProcessItemInheritAttrs = {
  type: ProcessItemType.InheritAttrs;
  value: boolean;
  node: VerterASTNode;
};

export type ProcessItemImport = {
  type: ProcessItemType.Import;
} & ImportModule;

export type ProcessItemError = {
  type: ProcessItemType.Error;

  message: ItemErrorString;
} & LocationProcessItem;

export type ProcessItemWarning = {
  type: ProcessItemType.Warning;

  message: ItemWarningString;
} & LocationProcessItem;

export type ProcessItemMacroReturn = {
  type: ProcessItemType.MacroReturn;

  content: string;
};

export type ProcessItemMacroBinding = {
  type: ProcessItemType.MacroBinding;
  name: string;
  macro: string;
  // originalName?: string;

  /**
   * Is the macro type only or is object.
   * For example, `defineProps<Type>()` vs `defineProps({})`
   * if has both then it's `type` eg: `defineProps<Type>({a: string})`
   * will be isType = true
   */
  isType?: boolean;

  /**
   * variable name for type
   */
  typeName?: string | null | undefined;
  /**
   * variable name for value
   */
  valueName?: string | null | undefined;
  /**
   * variable name for object
   */
  objectName?: string | null | undefined;

  node: VerterASTNode;
};

export type ProcessItemDefineModel = {
  type: ProcessItemType.DefineModel;
  /**
   * model name
   */
  name: string;

  /**
   * variableName
   */
  varName: string | null;

  node: VerterASTNode;

  /**
   * Is the macro type only or is object.
   * For example, `defineProps<Type>()` vs `defineProps({})`
   * if has both then it's `type` eg: `defineProps<Type>({a: string})`
   * will be isType = true
   */
  isType?: boolean;

  /**
   * variable name for type
   */
  typeName?: string | null | undefined;
  /**
   * variable name for value
   */
  valueName?: string | null | undefined;
  /**
   * variable name for object
   */
  objectName?: string | null | undefined;
};

export type ItemErrorString = "";
export type ItemWarningString =
  | "NO_EXPRESSION_VMODEL"
  | "MACRO_NOT_IN_SETUP"
  | "INVALID_DEFINE_OPTIONS"
  | "INVALID_WITH_DEFAULTS_DEFINE_PROPS_WITH_OBJECT_ARG";

export type ProcessItemBinding = {
  type: ProcessItemType.Binding;
  name: string;

  originalName?: string;

  item: ScriptItem;
  node: VerterASTNode;
};

export type ProcessItemOptions = {
  type: ProcessItemType.Options;
  /**
   * usually the function call node
   */
  node: VerterASTNode;
  /**
   * actual object expressions
   */
  expression: ObjectExpression | IdentifierName;
};

export type ProcessItem =
  | ProcessItemImport
  | ProcessItemError
  | ProcessItemWarning
  | ProcessItemBinding
  | ProcessItemOptions
  | ProcessItemMacroBinding
  | ProcessItemDefineModel
  | ProcessItemMacroReturn
  | ProcessItemInheritAttrs;

export type LocationProcessItem = {
  start: number;
  end: number;

  node?: VerterASTNode | Node;
};

export type ImportModule = {
  from: string;
  items?: ImportItem[];
  asType?: boolean;
  node?: VerterASTNode;
};

export type ImportItem = {
  name: string;
  alias?: string;
  type?: boolean;
  node?: VerterASTNode;
};
