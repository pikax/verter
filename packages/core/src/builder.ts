import { ParseScriptContext, PluginOption } from "./plugins/types.js";
import type { CompilerOptions } from "@vue/compiler-core";
import {
  MagicString,
  SFCDescriptor,
  SFCParseResult,
  SFCScriptCompileOptions,
  compileScript,
  parse,
} from "@vue/compiler-sfc";

import { defaultPlugins } from "./plugins/index.js";
import { pluginsToLocations } from "./utils/plugin.js";
import { extractBlocksFromDescriptor } from "./utils/sfc/sfc.js";

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

export function createBuilder(config?: Partial<BuilderOptions>) {
  const plugins: PluginOption[] = [
    ...defaultPlugins,
    ...(config?.plugins ?? []),
  ];

  return {
    preProcess(filename: string, source: string, ignoreScript = false) {
      // add empty script at the end
      if (
        source.indexOf("</script>") === -1 &&
        source.indexOf("<script ") === -1 &&
        source.indexOf("<script>") === -1
      ) {
        source += `\n<script></script>\n`;
      }

      const parsed = parse(source, {
        filename,
        sourceMap: true,
        ignoreEmpty: false,
      });

      const compiled =
        !ignoreScript &&
        (!!parsed.descriptor.scriptSetup || !!parsed.descriptor.script)
          ? compileScriptSafe(parsed.descriptor, {
              id: filename,
              genDefaultAs: "____VERTER_COMP_OPTION__",
              ...config?.vue?.compiler,
              // sourceMap: true,
              sourceMap: false,
              // globalTypeFiles: []
              isProd: true,
            })
          : null;
      const blocks = extractBlocksFromDescriptor(parsed.descriptor);

      const context = {
        filename,
        id: filename,
        isSetup: Boolean(compiled?.setup),
        isAsync: false,
        sfc: parsed,
        script: compiled,
        generic: compiled?.attrs.generic as string,
        template: parsed.descriptor.template,
        s: new MagicString(source),
        blocks,
      } satisfies ParseScriptContext;

      // if (!context.script) throw new Error("No script found");

      const locations = pluginsToLocations(plugins, context);

      return {
        locations,
        context,
      };
    },

    fromCompiled(result: SFCParseResult, ignoreScript: boolean = false) {
      const compiled =
        !ignoreScript &&
        (!!result.descriptor.scriptSetup || !!result.descriptor.script)
          ? compileScriptSafe(result.descriptor, {
              id: result.descriptor.filename,
              genDefaultAs: "____VERTER_COMP_OPTION__",
              ...config?.vue?.compiler,
              // sourceMap: true,
              sourceMap: false,
              // globalTypeFiles: []
              isProd: true,
            })
          : null;
      const blocks = extractBlocksFromDescriptor(result.descriptor);

      const context = {
        filename: result.descriptor.filename,
        id: result.descriptor.filename,
        isSetup: Boolean(compiled?.setup),
        isAsync: false,
        sfc: result,
        script: compiled,
        generic: compiled?.attrs.generic as string,
        template: result.descriptor.template,
        s: new MagicString(result.descriptor.source),
        blocks,
      } satisfies ParseScriptContext;

      // if (!context.script) throw new Error("No script found");

      const locations = pluginsToLocations(plugins, context);

      return {
        locations,
        context,
      };
    },

    process(_filename, _source) {
      // TODO
      return {} as any;
    },

    //     process(filename: string, source: string) {
    //       const parsed = parse(source, {
    //         filename,
    //         ignoreEmpty: true,
    //       });

    //       const compiled =
    //         !!parsed.descriptor.scriptSetup || !!parsed.descriptor.script
    //           ? compileScript(parsed.descriptor, {
    //               id: filename,
    //               // genDefaultAs: "GEN_COMP",
    //               ...config?.vue?.compiler,
    //               sourceMap: true,
    //             })
    //           : null;

    //       const context = {
    //         filename,
    //         id: filename,
    //         isSetup: Boolean(compiled?.setup),
    //         isAsync: false /* This will be populated by plugins */,
    //         sfc: parsed,
    //         script: compiled,
    //         generic: compiled?.attrs.generic.toString(),
    //         template: parsed.descriptor.template,
    //         s: new MagicString(source),
    //       } satisfies ParseScriptContext;

    //       // if (!context.script) throw new Error("No script found");

    //       const locations = pluginsToLocations(plugins, context);

    //       // todo move this away
    //       return this.finalise(locations, context);

    //       //   const processed = [...processPlugins(locations, plugins, context)];
    //     },
    //     finalise(map: LocationByType, context: ParseScriptContext) {
    //       const importsArray =
    //         map[LocationType.Import] ?? (map[LocationType.Import] = []);

    //       const _declarations =
    //         map[LocationType.Declaration] ?? (map[LocationType.Declaration] = []);

    //       const declarations = _declarations
    //         .filter((x) => x.generated)
    //         .reduce((prev, curr) => {
    //           if (!curr.declaration.name) {
    //             return `${prev}\n${curr.declaration.content}`;
    //           }

    //           return `${prev}\n${curr.declaration.type ?? "const"} ${
    //             curr.declaration.name
    //           } = ${curr.declaration.content};`;
    //         }, "");

    //       const notGenerated = _declarations
    //         .filter((x) => !x.generated)
    //         .reduce((prev, curr) => {
    //           if (!curr.declaration.name) {
    //             return `${prev}\n${curr.declaration.content}`;
    //           }

    //           return `${prev}\n${curr.declaration.type ?? "const"} ${
    //             curr.declaration.name
    //           } = ${curr.declaration.content};`;
    //         }, "");

    //       const _props = map[LocationType.Props]
    //         ?.map((x) => {
    //           if (x.properties) {
    //             return `{ ${x.properties
    //               .map((x) => `${x.name}: ${x.content}`)
    //               .join(", ")} }`;
    //           } else {
    //             return x.content;
    //           }
    //         })
    //         .join(" & ");

    //       const _emits = map[LocationType.Emits]
    //         ?.map((x) => {
    //           if (x.properties) {
    //             return `{ ${x.properties
    //               .map((x) => `${x.name}: [${x.content}]`)
    //               .join(", ")} }`;
    //           } else {
    //             return x.content;
    //           }
    //         })
    //         .join(" & ");

    //       const _expose = map[LocationType.Expose]
    //         ?.map((x) => x.content)
    //         .join(" & ");

    //       const exposeContent = _expose
    //         ? `
    //         function __exposeResolver${
    //           context.generic ? `<${context.generic}>` : ""
    //         }() {
    //           ${notGenerated}

    //           return ${_expose};
    //         }
    //       `
    //         : "";

    //       const _template = map[LocationType.Template]
    //         ?.map((x) => x.content)
    //         .join(" \n ");
    //       // const _templateCtx = map[]

    //       importsArray.push({
    //         type: LocationType.Import,
    //         node: undefined,
    //         from: "vue",
    //         items: [
    //           {
    //             name: "unref",
    //             // type: true,
    //           },
    //           {
    //             name: "ComponentExpectedProps",
    //             type: true,
    //           },
    //           {
    //             name: "renderList",
    //             // type: true,
    //           },
    //         ],
    //       });

    //       function getTemplate() {
    //         if (!_template) return "";

    //         const declared = new Set<string>();

    //         // used to spread the props
    //         let propsName = `({} as ComponentExpectedProps<__COMP__${
    //           genericNames ? `<${genericNames.join(",")}>` : ""
    //         }>)`;

    //         const propsProps = new Set<string>();

    //         const declarations = _declarations
    //           .filter((x) => !x.generated)
    //           .map((x) => {
    //             switch (x.node.type) {
    //               case "VariableDeclaration": {
    //                 return x.node.declarations
    //                   .map((x) => {
    //                     // TODO handle withdefaults too
    //                     if (x.init?.type === "CallExpression") {
    //                       if (x.init.callee.name === "defineProps") {
    //                         propsName = x.id.name;

    //                         // TODO add argument support
    //                         if (x.init.arguments) {
    //                         }

    //                         x.init.typeParameters?.params?.[0]?.members?.forEach(
    //                           (x) => propsProps.add(x.key.name)
    //                         );
    //                       }
    //                     } else if (x.id.type === "ObjectPattern") {
    //                       return x.id.properties.map((x) => x.key.name).join(", ");
    //                     }
    //                     return x.id.name;
    //                   })
    //                   .filter(Boolean)
    //                   .join(", ");
    //               }
    //               case "FunctionDeclaration": {
    //                 return x.node.id.name;
    //               }
    //               case "EnumDeclaration": {
    //                 return x.node.id.name;
    //               }
    //               case "ClassDeclaration": {
    //                 return x.node.id.name;
    //               }
    //               default: {
    //                 return "";
    //               }
    //             }
    //           })
    //           .filter(Boolean)
    //           .forEach((x) => declared.add(x));

    //         const contextVars = new Set(
    //           [
    //             ...(map[LocationType.Props]?.flatMap((x) =>
    //               x.properties?.map((p) => p.name)
    //             ) ?? []),
    //             ...propsProps,
    //             ...declared,
    //           ].filter(Boolean)
    //         );

    //         const imports = map[LocationType.Import]?.reduce((prev, curr) => {
    //           if (curr.generated === false) {
    //             prev.push(...curr.items.map((x) => x.alias ?? x.name));
    //           }
    //           return prev;
    //         }, []);

    //         return _template
    //           ? `
    //         function __templateResolver${
    //           context.generic ? `<${context.generic}>` : ""
    //         }() {
    //           const ctx = ()=> {
    //             ${notGenerated}

    //             return {
    //               ${
    //                 context.isSetup && imports.length > 0
    //                   ? imports.join(", ") + ", "
    //                   : ""
    //               }
    //               ...({} as ComponentInstance<__COMP__${
    //                 genericNames ? `<${genericNames.join(",")}>` : ""
    //               }>),
    //               ...${propsName},
    //               ${Array.from(declared)
    //                 .map((x) => [x, `unref(${x})`].join(": "))
    //                 .join(", ")}
    //             }
    //           }

    //           const _ctx = ctx();
    //           const _comp = {} as any as ExtractRenderComponents<typeof _ctx>;

    //           return (
    //             <>
    //             ${_template}
    //             </>
    //           );
    //         }
    //         `
    //           : "";
    //       }

    //       const emits = _emits ? `DeclareEmits<${_emits}>` : "{}";
    //       const props =
    //         [_props, emits && `EmitsToProps<${emits}>`]
    //           .filter(Boolean)
    //           .join(" & ") || "{}";

    //       const slots =
    //         map[LocationType.Slots]?.map((x) => x.content).join(" & ") || "{}";

    //       const genericDeclaration = context.generic ? map.generic?.[0] : undefined;

    //       function getGenericComponentName(name: string) {
    //         return "_VUE_TS__" + name;
    //       }

    //       function replaceComponentNameUsage(name: string, content: string) {
    //         const regex = new RegExp(`\\b${name}\\b`, "g");
    //         return content.replace(regex, getGenericComponentName(name));
    //       }

    //       const genericNames = genericDeclaration
    //         ? genericDeclaration.items.map((x) => x.name)
    //         : undefined;

    //       function sanitiseGenericNames(content: string | null | undefined) {
    //         if (!content) return content;
    //         return genericNames
    //           ? genericNames.reduce((prev, cur) => {
    //               return replaceComponentNameUsage(cur, prev);
    //             }, content)
    //           : content;
    //       }

    //       const CompGeneric = genericDeclaration
    //         ? genericDeclaration.items
    //             .map((x) => {
    //               const name = getGenericComponentName(x.name);
    //               const constraint = sanitiseGenericNames(x.constraint);
    //               const defaultType = sanitiseGenericNames(x.default);

    //               return [
    //                 name,
    //                 constraint ? `extends ${constraint}` : undefined,
    //                 `= ${defaultType || "any"}`,
    //               ]
    //                 .filter(Boolean)
    //                 .join(" ");
    //             })
    //             .join(", ")
    //         : undefined;
    //       const InstanceGeneric = genericDeclaration
    //         ? genericDeclaration.items
    //             .map((x) => {
    //               const name = x.name;
    //               const constraint =
    //                 x.constraint || getGenericComponentName(x.name);
    //               const defaultType = x.default || getGenericComponentName(x.name);

    //               return [
    //                 name,
    //                 constraint ? `extends ${constraint}` : undefined,
    //                 `= ${defaultType || "any"}`,
    //               ]
    //                 .filter(Boolean)
    //                 .join(" ");
    //             })
    //             .join(", ")
    //         : undefined;

    //       // if is generic don't use the __PROPS__ since it won't have the correct type
    //       const genericOrProps = InstanceGeneric
    //         ? `{ new<${InstanceGeneric}>(): { $props: ${props || "{}"}, $emit: ${
    //             emits || "{}"
    //           } , $children: ${slots || "{}"}, $data: ${
    //             exposeContent
    //               ? `ReturnType<typeof __exposeResolver<${genericDeclaration!.items
    //                   .map((x) => x.name)
    //                   .join(", ")}>>`
    //               : "__DATA__"
    //           }  } }`
    //         : "__PROPS__";
    //       // TODO better resolve the final variable names, especially options
    //       // because it relying on the plugin to build
    //       const declareComponent = `type __COMP__${
    //         CompGeneric ? `<${CompGeneric}>` : ""
    //       } = DeclareComponent<${genericOrProps}, __DATA__, __EMITS__, __SLOTS__,  "setup" extends keyof Type__options ? Type__options & { setup: () => {} } : Type__options >`;

    //       (map[LocationType.Export] ?? (map[LocationType.Export] = [])).push({
    //         type: LocationType.Export,
    //         node: undefined as any,
    //         item: {
    //           default: true,
    //           name: "__options",
    //           alias: "unknown as __COMP__",
    //           type: true,
    //         },
    //       });

    //       const exports = map[LocationType.Export]?.reduce((prev, curr) => {
    //         return `${prev}\nexport ${curr.item.default ? "default" : "const"} ${
    //           curr.item.default
    //             ? curr.item.content || curr.item.name
    //             : curr.item.name
    //         }${curr.item.alias ? " as " + curr.item.alias : ""};`;
    //       }, "");

    //       importsArray.push({
    //         type: LocationType.Import,
    //         node: undefined as any,
    //         from: "vue",
    //         items: [
    //           {
    //             name: "DeclareComponent",
    //             type: true,
    //           },
    //         ],
    //       });

    //       if (emits) {
    //         importsArray.push({
    //           type: LocationType.Import,
    //           node: undefined as any,
    //           from: "vue",
    //           items: [
    //             { name: "DeclareEmits", type: true },
    //             {
    //               name: "EmitsToProps",
    //               type: true,
    //             },
    //           ],
    //         });
    //       }
    //       if (map[LocationType.Slots]?.length) {
    //         importsArray.push({
    //           type: LocationType.Import,
    //           node: undefined as any,
    //           from: "vue",
    //           items: [{ name: "SlotsType", type: true }],
    //         });
    //       }

    //       // TODO should group imports
    //       const imports = map[LocationType.Import]?.reduce((prev, curr) => {
    //         if (!curr.items?.length) {
    //           if (
    //             curr.from.startsWith("import") &&
    //             curr.from.indexOf(" from ") > 0
    //           ) {
    //             return `${prev}\n${curr.from}`;
    //           }
    //           return prev;
    //         }
    //         return `${prev}\nimport { ${curr.items
    //           .map((it) => (it.alias ? `${it.name} as ${it.alias}` : it.name))
    //           .filter(Boolean)
    //           .join(", ")} } from '${curr.from}';`;
    //       }, "");
    //       const templateContent = getTemplate();

    //       return `${imports}\n

    // ${declarations}\n

    // // expose
    // ${exposeContent}

    // // template
    // ${templateContent}

    // ${
    //   context.generic
    //     ? [
    //         `type __DATA__ = {};`,
    //         `type __EMITS__ = {};`,
    //         `type __SLOTS__ = {};`,
    //       ].join("\n")
    //     : [
    //         `type __PROPS__ = ${props};`,
    //         `type __DATA__ = {};`,
    //         `type __EMITS__ = ${emits};`,
    //         `type __SLOTS__ = ${slots};`,
    //       ].join("\n")
    // }

    // ${declareComponent}

    // ${exports || ""}
    //         `;
    //     },
  };
}
