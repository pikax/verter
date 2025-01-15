import { MagicString } from "@vue/compiler-sfc";
import { ParsedBlock } from "../parser/types";
import { VerterASTNode } from "../parser/ast";
import { Node } from "@vue/compiler-core";
import { GenericInfo } from "../parser/script/generic";

export type ProcessContext = {
  filename: string;
  s: MagicString;

  isAsync: boolean;
  generic: GenericInfo | null;

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

export type ItemErrorString = "";
export type ItemWarningString = "NO_EXPRESSION_VMODEL";

export type ProcessItem =
  | ProcessItemImport
  | ProcessItemError
  | ProcessItemWarning;

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
