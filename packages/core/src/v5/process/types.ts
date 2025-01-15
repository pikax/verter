import { MagicString } from "@vue/compiler-sfc";
import { ParsedBlock } from "../parser/types";
import {
  TemplateItem,
  TemplateItemByType,
  TemplateTypes,
} from "../parser/template/types";

export type ProcessContext = {
  filename: string;
  s: MagicString;

  isAsync: boolean;
  // generic: GenericInfo | undefined;
  generic: string | undefined;

  blocks: ParsedBlock[];
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
