import {
  MagicString,
  SFCDescriptor,
  SFCParseOptions,
  SFCScriptCompileOptions,
  SFCStyleCompileOptions,
  compileScript,
  parse,
} from "@vue/compiler-sfc";
import { defaultPlugins, ParseScriptContext, PluginOption } from "./plugins";
import { pluginsToLocations } from "./utils/plugin";
import { extractBlocksFromDescriptor } from "./utils/sfc";

function compileScriptSafe(
  descriptor: SFCDescriptor,
  options: SFCScriptCompileOptions
) {
  try {
    return compileScript(descriptor, options);
  } catch (e) {
    console.error("failed compiling script", e);
    return null;
  }
}

export function createContext(
  source: string,
  filename: string = "temp.vue",
  compile: boolean | SFCStyleCompileOptions = false,
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

  const script = compile
    ? compileScriptSafe(parsed.descriptor, {
        id: filename,
        genDefaultAs: "____VERTER_COMP_OPTION__",
        // sourceMap: true,
        sourceMap: false,
        inlineTemplate: true,
        propsDestructure: true,
        // globalTypeFiles: []
        isProd: true,
        babelParserPlugins: [],
        hoistStatic: false,
        templateOptions: {
          isProd: true,
          ssr: false,
          // there's no need for the template to be compiled
          compiler: {
            compile(src, opts) {
              return {
                ast: null as any,
                code: `{}`,
                preamble: "",
              };
            },
            parse(src, opts) {
              return {} as any;
            },
          },
          compilerOptions: {
            // isTS: false,
            // prefixIdentifiers: true,
            // mode: 'module',
            expressionPlugins: ["typescript"],
          },
        },
        ...(typeof compile === "object" ? compile : {}),
      })
    : parsed.descriptor.scriptSetup ?? parsed.descriptor.script;

  const blocks = extractBlocksFromDescriptor(parsed.descriptor);

  const context = {
    id: filename,
    filename,
    isSetup: !!parsed.descriptor.scriptSetup,
    isAsync: false,
    sfc: parsed,
    script,
    generic: script.attrs.generic as string,
    template: parsed.descriptor.template,
    s: new MagicString(source),
    blocks,
  } satisfies ParseScriptContext;

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
