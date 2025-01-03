import {
  MagicString,
  type SFCParseOptions,
  SFCTemplateBlock,
  parse,
} from "@vue/compiler-sfc";
import {
  cleanHTMLComments,
  extractBlocksFromDescriptor,
  findBlockLanguage,
  VerterSFCBlock,
} from "../utils/index.js";
import { parseScript } from "./script/script.js";
import { parseAST } from "./ast/ast.js";
import { parseTemplate } from "./template/template.js";
import {
  ParsedBlock,
  ParsedBlockScript,
  ParsedBlockTemplate,
  ParsedBlockUnknown,
} from "./types.js";

// import { parseScript } from "./script/index.js";

export type ParserResult = {
  filename: string;

  s: MagicString;

  isAsync: boolean;
  // generic: GenericInfo | undefined;
  generic: string | undefined;

  blocks: ParsedBlock[];
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

const JS_AST_LANGUAGES = new Set(["ts", "tsx", "js", "jsx"]);

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
  let generic: string | undefined = undefined;
  let isAsync = false;

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

  const blocks = extractBlocksFromDescriptor(sfcParse.descriptor).map((x) => {
    const languageId = findBlockLanguage(x);
    switch (languageId) {
      case "vue":
        const ast = parseTemplate(
          (x.block as SFCTemplateBlock).ast!,
          x.block.content
        );

        return {
          type: "template",
          lang: languageId,
          block: x,
          result: ast,
        } as ParsedBlockTemplate;
      case "ts":
      case "tsx": {
        if (
          x.block.attrs.generic &&
          typeof x.block.attrs.generic === "string"
        ) {
          generic = x.block.attrs.generic;
        }
      }
      case "js":
      case "jsx": {
        const ast = parseAST(x.block.content, filename);

        const r = parseScript(ast, x.block.content);
        isAsync = r.isAsync;

        return {
          type: "script",
          lang: languageId,
          block: x,
          result: r,
          isMain:
            x.block ===
            (sfcParse.descriptor.scriptSetup || sfcParse.descriptor.script),
        } as ParsedBlockScript;
      }
      default:
        return {
          type: x.tag.type,
          lang: languageId,
          block: x,
          result: null,
        } as ParsedBlockUnknown;
    }
  });

  return {
    filename,

    s,
    generic,
    isAsync,

    blocks,
  };
}
