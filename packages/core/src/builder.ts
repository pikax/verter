import {
  LocationByType,
  LocationType,
  ParseScriptContext,
  PluginOption,
  TypeLocation,
  WalkResult,
} from "./plugins/types.js";
import type { CompilerOptions } from "@vue/compiler-core";
import { compileScript, parse } from "@vue/compiler-sfc";

import { defaultPlugins } from "./plugins/index.js";
import { Statement } from "@babel/types";

export interface Builder {
  build(): void;
}

export interface BuilderOptions {
  plugins: PluginOption[];

  vue: {
    compiler: CompilerOptions;
  };

  // enables legacy mode, used mainly for before v3.5
  legacy: boolean;
}

export function createBuilder(config?: Partial<BuilderOptions>) {
  const plugins: PluginOption[] = [
    ...defaultPlugins,
    ...(config?.plugins ?? []),
  ];

  return {
    process(filename: string, source: string) {
      const parsed = parse(source, {
        filename,
      });
      // TODO we should still process it
      if (!parsed.descriptor.scriptSetup && !parsed.descriptor.script)
        return "";

      const compiled = compileScript(parsed.descriptor, {
        id: filename,
        ...config?.vue?.compiler,
      });

      const context = {
        filename,
        id: filename,
        isSetup: Boolean(compiled.setup),
        sfc: parsed,
        script: compiled,
        generic: compiled.attrs.generic,
      } satisfies ParseScriptContext;

      if (!context.script) throw new Error("No script found");

      // create a map
      const locations = [
        ...processPlugins(plugins, context),
        ...walkPlugins(plugins, context),
      ].reduce((prev, curr) => {
        if (!curr) return prev;
        const type = curr.type;
        if (!prev[type]) prev[type] = [];
        prev[type]!.push(curr);
        return prev;
      }, {} as Record<LocationType, TypeLocation[]>) as unknown as LocationByType;

      // todo move this away
      return this.finalise(locations, context);

      //   const processed = [...processPlugins(locations, plugins, context)];
    },
    finalise(map: LocationByType, context: ParseScriptContext) {
      const importsArray =
        map[LocationType.Import] ?? (map[LocationType.Import] = []);

      const _declarations =
        map[LocationType.Declaration] ?? (map[LocationType.Declaration] = []);

      const declarations = _declarations
        .filter((x) => x.generated)
        .reduce((prev, curr) => {
          if (!curr.declaration.name) {
            return `${prev}\n${curr.declaration.content}`;
          }

          return `${prev}\n${curr.declaration.type ?? "const"} ${
            curr.declaration.name
          } = ${curr.declaration.content};`;
        }, "");

      const notGenerated = _declarations
        .filter((x) => !x.generated)
        .reduce((prev, curr) => {
          if (!curr.declaration.name) {
            return `${prev}\n${curr.declaration.content}`;
          }

          return `${prev}\n${curr.declaration.type ?? "const"} ${
            curr.declaration.name
          } = ${curr.declaration.content};`;
        }, "");

      const _props = map[LocationType.Props]
        ?.map((x) => {
          if (x.properties) {
            return `{ ${x.properties
              .map((x) => `${x.name}: ${x.content}`)
              .join(", ")} }`;
          } else {
            return x.content;
          }
        })
        .join(" & ");

      const _emits = map[LocationType.Emits]
        ?.map((x) => {
          if (x.properties) {
            return `{ ${x.properties
              .map((x) => `${x.name}: [${x.content}]`)
              .join(", ")} }`;
          } else {
            return x.content;
          }
        })
        .join(" & ");

      const _expose = map[LocationType.Expose]
        ?.map((x) => x.content)
        .join(" & ");

      const exposeContent = _expose
        ? `
        function __exposeResolver${
          context.generic ? `<${context.generic}>` : ""
        }() {
          ${notGenerated}

          return ${_expose};
        }
      `
        : "";

      const emits = _emits ? `DeclareEmits<${_emits}>` : undefined;
      const props = [_props, emits && `EmitsToProps<${emits}>`]
        .filter(Boolean)
        .join(" & ");

      const slots =
        map[LocationType.Slots]?.map((x) => x.content).join(" & ") || "{}";

      // if is generic don't use the __PROPS__ since it won't have the correct type
      const genericOrProps = context.generic
        ? `{ new<${context.generic}>(): { $props: ${props || "{}"}, $emit: ${
            emits || "{}"
          } , $children: ${slots || "{}"}, $data: ${
            exposeContent
              ? `ReturnType<typeof __exposeResolver<${context.generic}>>`
              : "__DATA__"
          }  } }`
        : "__PROPS__";

      // TODO better resolve the final variable names, especially options
      // because it relying on the plugin to build
      const declareComponent = `type __COMP__${
        context.generic ? `<${context.generic}>` : ""
      } = DeclareComponent<${genericOrProps}, __DATA__, __EMITS__, __SLOTS__, Type__options>`;

      (map[LocationType.Export] ?? (map[LocationType.Export] = [])).push({
        type: LocationType.Export,
        node: undefined as any,
        item: {
          default: true,
          name: "__options",
          alias: "unknown as __COMP__",
          type: true,
        },
      });

      const exports = map[LocationType.Export]?.reduce((prev, curr) => {
        return `${prev}\nexport ${curr.item.default ? "default" : "const"} ${
          curr.item.default
            ? curr.item.content || curr.item.name
            : curr.item.name
        }${curr.item.alias ? " as " + curr.item.alias : ""};`;
      }, "");

      importsArray.push({
        type: LocationType.Import,
        node: undefined as any,
        from: "vue",
        items: [
          {
            name: "DeclareComponent",
            type: true,
          },
        ],
      });

      if (_emits) {
        importsArray.push({
          type: LocationType.Import,
          node: undefined as any,
          from: "vue",
          items: [
            { name: "DeclareEmits", type: true },
            {
              name: "EmitsToProps",
              type: true,
            },
          ],
        });
      }
      if (map[LocationType.Slots]?.length) {
        importsArray.push({
          type: LocationType.Import,
          node: undefined as any,
          from: "vue",
          items: [{ name: "SlotsType", type: true }],
        });
      }

      // TODO should group imports
      const imports = map[LocationType.Import]?.reduce((prev, curr) => {
        if (!curr.items?.length) {
          if (
            curr.from.startsWith("import") &&
            curr.from.indexOf(" from ") > 0
          ) {
            return `${prev}\n${curr.from}`;
          }
          return prev;
        }
        return `${prev}\nimport { ${curr.items
          .map((it) => it.name)
          .filter(Boolean)
          .join(", ")} } from '${curr.from}';`;
      }, "");

      return `${imports}\n

${declarations}\n

// expose
${exposeContent}

${
  context.generic
    ? [
        `type __DATA__ = {};`,
        `type __EMITS__ = {};`,
        `type __SLOTS__ = {};`,
      ].join("\n")
    : [
        `type __PROPS__ = ${props};`,
        `type __DATA__ = {};`,
        `type __EMITS__ = ${emits};`,
        `type __SLOTS__ = ${slots};`,
      ].join("\n")
}

${declareComponent}


${exports || ""}
        `;
    },
  };
}

function* walkPlugins(plugins: PluginOption[], context: ParseScriptContext) {
  yield* runPlugins((plugin) => plugin.walk, plugins, context);
}
function* processPlugins(plugins: PluginOption[], context: ParseScriptContext) {
  for (const plugin of plugins) {
    if (!plugin.process) continue;
    const result = plugin.process(context);
    if (!result) continue;
    if (Array.isArray(result)) {
      yield* result;
    } else {
      yield result;
    }
  }
}

function* runPlugins(
  cb: (
    plugin: PluginOption
  ) =>
    | undefined
    | ((state: Statement, context: ParseScriptContext) => void | WalkResult),
  plugins: PluginOption[],
  context: ParseScriptContext
) {
  if (!context.script) return;
  const ast = context.script.scriptSetupAst ?? context.script.scriptAst;
  if (!ast) return;
  for (const statement of ast) {
    for (const plugin of plugins) {
      const result = cb(plugin)?.(statement, context);
      if (!result) continue;
      if (Array.isArray(result)) {
        yield* result;
      } else {
        yield result;
      }
    }
  }
}
