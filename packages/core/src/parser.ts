import {
  MagicString,
  SFCBlock,
  SFCDescriptor,
  SFCParseOptions,
  SFCScriptCompileOptions,
  SFCStyleCompileOptions,
  compileScript,
  parse,
  walk,
} from "@vue/compiler-sfc";
import {
  walk as templateWalk,
  VerterNode,
} from "./plugins/template/v2/walk/index.js";
import { defaultPlugins, ParseScriptContext, PluginOption } from "./plugins";
import { pluginsToLocations } from "./utils/plugin";
import {
  extractBlocksFromDescriptor,
  findBlockLanguage,
  VerterSFCBlock,
} from "./utils/sfc";

import oxc from "oxc-parser";
import acorn from "acorn-loose";
import {
  isFunctionType,
  NodeTypes,
  walkIdentifiers,
  Node,
  ElementTypes,
} from "@vue/compiler-core";
import { PrefixSTR } from "./plugins/template/v2/transpile";
import { Identifier, Node as BabelNode, is } from "@babel/types";

type AST = ReturnType<typeof acorn.parse>;

export type VerterASTBlock = VerterSFCBlock & {
  /**
   * This will trigger a parse of the block
   * @returns `AST` or `false` if not a valid block to parse
   */
  readonly ast: AST | false;
};

export type VerterIdentifier = {
  loc: {
    start: number;
    end: number;
  };
  node: Identifier | Node;
  content: string;

  type: "binding" | "component";
};

export type ParseContext = {
  filename: string;
  source: string;
  s: MagicString;

  sfc: SFCDescriptor;

  /**
   * This will trigger a parse of the main script block
   */
  readonly isAsync: boolean;
  readonly isSetup: boolean;

  /**
   * Template blocks, calling `ast` will parse try to parse the block
   */
  blocks: VerterASTBlock[];

  generic: GenericInfo | undefined;

  templateIdentifiers: VerterIdentifier[];
};

function parseASTFallback(source: string) {
  const ast = acorn.parse(source, { ecmaVersion: "latest" });
  return ast;
}

export function parseAST(
  source: string,
  sourceFilename: string = "index.ts"
): AST {
  // handling non-ascii characters gives some odd nodes position in the AST
  const normaliseSource = source.replace(/[^\x00-\x7F]/g, "*");
  let ast: AST;
  try {
    const result = oxc.parseSync(normaliseSource, {
      sourceFilename,
    });

    if (result.errors.length) {
      // fallback to acorn parser if oxc parser failed
      // because acorn-loose is more lenient in handling errors
      throw result.errors;
    } else {
      ast = JSON.parse(result.program);
    }
  } catch (e) {
    console.error("oxc parser failed", e);
    ast = parseASTFallback(normaliseSource);
  }

  return ast;
}

export function createContext(
  source: string,
  filename: string = "temp.vue",
  options: Partial<SFCParseOptions> = {}
) {
  // add empty script at the end
  if (
    source.indexOf("</script>") === -1 &&
    source.indexOf("<script ") === -1 &&
    source.indexOf("<script>") === -1
  ) {
    source += `\n<script></script>\n`;
  }

  const parsed = parse(source, {
    ...options,
    filename,
    sourceMap: true,
    ignoreEmpty: false,
    templateParseOptions: {
      prefixIdentifiers: true,
      expressionPlugins: ["typescript"],
    },
  });

  const processedBlocksMap = new WeakMap<SFCBlock, AST | false>();

  const blocks: VerterASTBlock[] = extractBlocksFromDescriptor(
    parsed.descriptor
  ).map((x) => {
    return Object.assign(x, {
      get ast() {
        let ast = processedBlocksMap.get(x.block);
        if (ast === undefined) {
          const lang = findBlockLanguage(x);
          if (
            lang === "js" ||
            lang === "jsx" ||
            lang === "ts" ||
            lang === "tsx"
          ) {
            ast = parseAST(x.block.content, filename + "." + lang);
          } else {
            ast = false;
          }
          processedBlocksMap.set(x.block, ast);
        }
        return ast;
      },
    });
  });

  const isSetup = !!parsed.descriptor.scriptSetup;
  let isAsync = isSetup ? undefined : false;

  function resolveIsAsync() {
    if (isAsync !== undefined) return isAsync;

    const block = blocks.find((x) => x.block === parsed.descriptor.scriptSetup);

    const r = block.ast;
    if (!r) return false;

    walk(r, {
      enter(node) {
        if (isFunctionType(node) || isAsync) {
          this.skip();
        }

        if (node.type === "AwaitExpression") {
          isAsync = true;
          this.skip();
        }
      },
    });
    if (isAsync === undefined) isAsync = false;
    return isAsync;
  }

  let generic: GenericInfo | undefined | null = null;
  function resolveGeneric() {
    if (generic !== null) return generic;
    const block = blocks.find(
      (x) =>
        x.block === (parsed.descriptor.scriptSetup || parsed.descriptor.script)
    );
    generic = parseGeneric(block.block.attrs.generic);
    return generic;
  }

  let identifiers: VerterIdentifier[] | null = null;
  function resolveTemplateIdentifiers() {
    if (identifiers !== null) return identifiers;
    if (!parsed.descriptor.template?.ast) {
      return (identifiers = []);
    }
    identifiers = [];
    templateWalk(
      parsed.descriptor.template.ast,
      {
        enter(item, _, context) {
          const foundIdentifiers: string[] = [];

          function processNodeAst(node: VerterNode, add = true) {
            if ("ast" in node && node.ast) {
              const toReturn: VerterIdentifier[] = [];
              const ast = node.ast as BabelNode;
              walkIdentifiers(
                ast,
                (n, prent, parentStack, isReference, isLocal) => {
                  if (!isReference || (add && isLocal)) return;
                  const start = node.loc.start.offset - 1 + n.loc.start.index;
                  const len = n.end - n.start;
                  const end = start + len;
                  const content = n.name;

                  if (context.ignoredIdentifers.has(content)) return;
                  const identifier = {
                    loc: {
                      start,
                      end,
                    },
                    content,
                    node,
                    type: "binding",
                  };

                  if (add) {
                    // @ts-expect-error todo type
                    identifiers.push(identifier);
                  } else {
                    // @ts-expect-error todo type
                    toReturn.push(identifier);
                  }
                },
                true
              );

              if (add) {
                return true;
              }
              return toReturn;
            }
            return false;
          }

          function processNode(
            node: VerterNode,
            add = true,
            isArgumentNode = false
          ) {
            if (!node) return;
            switch (node.type) {
              case NodeTypes.SIMPLE_EXPRESSION: {
                if (node.isStatic) return;
                if (!processNodeAst(node)) {
                  const start = isArgumentNode
                    ? node.loc.start.offset + 1
                    : node.loc.start.offset;
                  const end = isArgumentNode
                    ? node.loc.end.offset - 1
                    : node.loc.end.offset;
                  const content = node.content;
                  const identifier = {
                    loc: {
                      start,
                      end,
                    },
                    content,
                    node: node,
                    type: "binding",
                  };
                  if (add) {
                    // @ts-expect-error todo type
                    identifiers.push(identifier);
                  }
                  return identifier;
                }
                break;
              }
              case NodeTypes.INTERPOLATION: {
                // @ts-expect-error todo type
                if (!processNodeAst(node.content)) {
                  identifiers.push({
                    loc: {
                      start: node.content.loc.start.offset,
                      end: node.content.loc.end.offset,
                    },
                    // @ts-expect-error not the correct type
                    content: node.content.content as string,
                    node: node.content,
                    type: "binding",
                  });
                }
                break;
              }
              case NodeTypes.ELEMENT: {
                if (node.tagType === ElementTypes.COMPONENT) {
                  identifiers.push({
                    loc: {
                      start: node.loc.start.offset + 1,
                      end: node.loc.start.offset + 1 + node.tag.length,
                    },
                    content: node.tag,
                    node,
                    type: "component",
                  } as VerterIdentifier);
                }

                for (const prop of node.props) {
                  if (
                    prop.name === "for" &&
                    "forParseResult" in prop &&
                    prop.forParseResult
                  ) {
                    // @ts-expect-error todo type
                    const value = processNode(prop.forParseResult.value, false);
                    // @ts-expect-error todo type
                    const key = processNode(prop.forParseResult.key, false);
                    // @ts-expect-error todo type
                    const index = processNode(prop.forParseResult.index, false);
                    // @ts-expect-error todo type
                    processNode(prop.forParseResult.source);

                    foundIdentifiers.push(
                      ...[value?.content, key?.content, index?.content].filter(
                        Boolean
                      )
                    );
                    // prop.forParseResult.;
                    // @ts-expect-error todo type
                  } else if (prop.rawName === "v-slot") {
                    // @ts-expect-error todo type
                    const found = processNodeAst(prop.exp, false);
                    if (found) {
                      // @ts-expect-error todo type
                      foundIdentifiers.push(...found.map((x) => x.content));
                    }
                  } else {
                    if (prop.type === NodeTypes.DIRECTIVE) {
                      // @ts-expect-error todo type
                      processNode(prop.arg, true, true);
                      // @ts-expect-error todo type
                      processNode(prop.exp);
                    }
                  }
                }
              }
            }
          }

          processNode(item);
          // }
          // if (node.type === "Identifier") {
          //   identifiers.push(node);
          // }

          return {
            ignoredIdentifers: new Set([
              ...context.ignoredIdentifers.values(),
              ...foundIdentifiers,
            ]),
          };
        },
      },
      {
        ignoredIdentifers: new Set(["$event"]),
      }
    );

    return identifiers;
  }

  const context = {
    filename,
    source,
    blocks,
    isSetup,

    s: new MagicString(source),
    sfc: parsed.descriptor,

    get isAsync() {
      return resolveIsAsync();
    },
    get generic() {
      return resolveGeneric();
    },

    get templateIdentifiers() {
      return resolveTemplateIdentifiers();
    },
  } satisfies ParseContext;
  return context;
}

export function parseLocations(
  context: ParseScriptContext,
  appendPlugins: PluginOption[] = []
) {
  const plugins = [...defaultPlugins, ...appendPlugins];
  const locations = pluginsToLocations(plugins, context);
  return locations;
}

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

export function parseGeneric(source: string | true, prefix = PrefixSTR("TS_")) {
  if (typeof source !== "string" || !source) return undefined;

  const genericCode = `type __GENERIC__<${source}> = {}`;
  const {
    body: [genericNode],
  } = parseAST(genericCode, "generic.ts");

  // TODO handle if generic is invalid, maybe do a parse with REGEX
  if (!("typeParameters" in genericNode) || !genericNode.typeParameters)
    return undefined;
  // @ts-expect-error not the correct type
  const params = genericNode.typeParameters?.params || [];

  function retrieveNodeString(node: any, source: string) {
    if (!node) return undefined;
    return source.slice(node.start, node.end);
  }

  const items = params.map((param, index) => ({
    name: param.name.name,
    content: retrieveNodeString(param, genericCode),
    constraint: retrieveNodeString(param.constraint, genericCode),
    default: retrieveNodeString(param.default, genericCode),
    index,
  }));

  function getGenericComponentName(name: string) {
    return prefix + name;
  }

  function replaceComponentNameUsage(name: string, content: string) {
    const regex = new RegExp(`\\b${name}\\b`, "g");
    return content.replace(regex, getGenericComponentName(name));
  }
  function sanitiseGenericNames(content: string | null | undefined) {
    if (!content) return content;
    return names
      ? names.reduce((prev, cur) => {
          return replaceComponentNameUsage(cur, prev);
        }, content)
      : content;
  }

  const names = items.map((x) => x.name);
  const sanitisedNames = names.map(sanitiseGenericNames);

  const declaration = items
    .map((x) => {
      const name = getGenericComponentName(x.name);
      const constraint = sanitiseGenericNames(x.constraint);
      const defaultType = sanitiseGenericNames(x.default);

      return [
        name,
        constraint ? `extends ${constraint}` : undefined,
        `= ${defaultType || "any"}`,
      ]
        .filter(Boolean)
        .join(" ");
    })
    .join(", ");

  return {
    source,
    names,
    sanitisedNames,
    declaration,
  } satisfies GenericInfo;
}
