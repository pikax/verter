import {
  MagicString,
  type SFCParseOptions,
  SFCScriptBlock,
  SFCTemplateBlock,
  parse,
} from "@vue/compiler-sfc";
import {
  cleanHTMLComments,
  extractBlocksFromDescriptor,
  findBlockLanguage,
  VerterSFCBlock,
} from "../utils/index.js";
import { parseScript, parseScriptBetter } from "./script/script.js";
import { parseAST } from "./ast/ast.js";
import { parseTemplate } from "./template/template.js";
import {
  ParsedBlock,
  ParsedBlockScript,
  ParsedBlockTemplate,
  ParsedBlockUnknown,
} from "./types.js";
import { parseGeneric, GenericInfo } from "./script/generic/index.js";

// import { parseScript } from "./script/index.js";

export type ParserResult = {
  filename: string;

  s: MagicString;

  isAsync: boolean;
  generic: GenericInfo | null;

  blocks: ParsedBlock[];

  isTS: boolean;
  isSetup: boolean;
};

export type VerterParserOptions = SFCParseOptions & {
  accessorPrefix: string;
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
  let generic: GenericInfo | null = null;
  let isAsync = false;
  let isTS = false;
  let isSetup = false;

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

  const blocks = extractBlocksFromDescriptor(sfcParse.descriptor)
    // sort template to last
    .sort((a, b) => {
      if (a.tag.type === "template") return 1;
      if (b.tag.type === "template") return -1;
      return 0;
    })
    .map((x) => {
      const languageId = findBlockLanguage(x);
      switch (languageId) {
        case "vue":
          const ast = parseTemplate(
            (x.block as SFCTemplateBlock).ast!,
            x.block.content,
            generic?.names
          );

          return {
            type: "template",
            lang: languageId,
            block: x,
            result: ast,
          } as ParsedBlockTemplate;
        case "ts":
        case "tsx": {
          isTS = true;
          if (x.tag.attributes.generic && x.tag.attributes.generic.value) {
            generic = parseGeneric(
              x.tag.attributes.generic.value.content,
              x.tag.attributes.generic.value.start
            );
          }
        }
        case "js":
        case "jsx": {
          const prepend = "".padStart(x.block.loc.start.offset, " ");
          const content = prepend + x.block.content;

          const ast = parseAST(content, filename + "." + languageId);
          const r = parseScriptBetter(ast, x.block.attrs);
          isAsync = r.isAsync;
          if (!isSetup) {
            isSetup = !!((x.block as SFCScriptBlock)?.setup ?? false);
          }

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
    isTS,
    isSetup,

    blocks,
  };
}
