import { MagicString, type SFCParseOptions, parse } from "@vue/compiler-sfc";
import {
  cleanHTMLComments,
  extractBlocksFromDescriptor,
  findBlockLanguage,
} from "../utils/index.js";

export type ParserResult = {
  filename: string;

  s: MagicString;

  isAsync: boolean;
  generic: GenericInfo | undefined;

  imports: any[];

  bindings: any[];
};

export type VerterParserOptions = SFCParseOptions & {
  accessorPrefix: string;
};

export type GenericInfo = {
  /**
   * Source of the generic
   *
   * @value
   *  ```ts
   *  T extends 'foo' | 'bar'
   * ```
   * @example
   * ```vue
   * <script generic="T extends 'foo' | 'bar'" setup lang="ts">
   * ```
   */
  source: string;

  /**
   * Used to prefix sanitised names
   * @default '__VERTER__TS__'
   */
  sanitisePrefix?: string;

  /**
   * The generic array names
   * @example
   * ```vue
   * <script generic="T extends 'foo' | 'bar', Comp" setup lang="ts">
   * ```
   * The names will be `['T', 'Comp']`
   */
  names: string[];

  /**
   * The sanitised names, prefixed with `sanitisePrefix`
   * @example
   * ```vue
   * <script generic="T extends 'foo' | 'bar', Comp" setup lang="ts">
   * ```
   * The names will be `['__VERTER__TS__T', '__VERTER__TS__Comp']`
   */
  sanitisedNames: string[];

  /**
   * Declaration generic, it contains the same constraints and default as `source`
   * but the names are sanitised and if no default provided it will default to `any`
   *
   * @example
   * ```vue
   * <script generic="T extends 'foo' | 'bar', Comp" setup lang="ts">
   * ```
   * The declaration will be the content
   * ```ts
   * <__VERTER__TS__T extends 'foo' | 'bar' = any, Comp = any>()=>{}
   * ```
   */
  declaration: string;
};

const AST_LANGUAGES = new Set(["ts", "tsx", "js", "jsx"]);

export function parser(
  source: string,
  filename: string = "temp.vue",
  options: Partial<VerterParserOptions> = {}
): ParserResult {
  {
    // add empty script at the end if no valid script available
    const noHTMLCommentSource = cleanHTMLComments(source);

    if (
      noHTMLCommentSource.indexOf("</script>") === -1 &&
      noHTMLCommentSource.indexOf("<script ") === -1 &&
      noHTMLCommentSource.indexOf("<script>") === -1
    ) {
      source += `\n<script></script>\n`;
    }
  }

  const s = new MagicString(source);
  const generic = undefined;
  const isAsync = false;
  const imports = [];
  const bindings = [];

  const sfcParse = parse(source, {
    ...options,
    filename,
    // sourceMap: true,
    ignoreEmpty: false,
    templateParseOptions: {
      prefixIdentifiers: true,
      expressionPlugins: ["typescript"],
    },
  });

  const descriptorBlocks = extractBlocksFromDescriptor(sfcParse.descriptor).map(
    (x) => {
      const languageId = findBlockLanguage(x);
      // const ast = AST_LANGUAGES.has(languageId) ? ;
    }
  );

  return {
    filename,

    s,
    generic,
    isAsync,

    imports,
    bindings,
  };
}
