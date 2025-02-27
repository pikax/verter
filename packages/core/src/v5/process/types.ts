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
   * Contains the component options, for example `defineOptions`
   */
  Options = "options",
}

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

export type ProcessItemMacroBinding = {
  type: ProcessItemType.MacroBinding;
  name: string;
  macro: string;
  originalName?: string;
};

export type ItemErrorString = "";
export type ItemWarningString =
  | "NO_EXPRESSION_VMODEL"
  | "MACRO_NOT_IN_SETUP"
  | "INVALID_DEFINE_OPTIONS";

export type ProcessItemBinding = {
  type: ProcessItemType.Binding;
  name: string;

  originalName?: string;

  item: ScriptItem;
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
  | ProcessItemMacroBinding;

export type LocationProcessItem = {
  start: number;
  end: number;

  node?: VerterASTNode | Node;
};

export type ImportModule = {
  from: string;
  items: ImportItem[];
  asType?: boolean;
};

export type ImportItem = {
  name: string;
  alias?: string;
  type?: boolean;
};
