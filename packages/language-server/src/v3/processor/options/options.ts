import {
  LocationByType,
  LocationType,
  parseLocations,
  ParseScriptContext,
} from "@verter/core";
import { getBlockFilename } from "../utils";
import { MagicString } from "vue/compiler-sfc";
import { transform } from "sucrase";
import { VerterSFCBlock } from "@verter/core/dist/utils/sfc";

export const OptionsExportName = "ComponentOptions";

export const BindingContextExportName = "BindingContext";

function genericProcess(
  context: ParseScriptContext,
  locations: LocationByType
) {
  if (!context.generic) {
    return undefined;
  }
  const genericDeclaration = locations.generic[0];
  if (!genericDeclaration) return undefined;

  function getGenericComponentName(name: string) {
    return "_VUE_TS__" + name;
  }

  function replaceComponentNameUsage(name: string, content: string) {
    const regex = new RegExp(`\\b${name}\\b`, "g");
    return content.replace(regex, getGenericComponentName(name));
  }

  const genericNames = genericDeclaration.items.map((x) => x.name);
  const sanitisedNames = genericNames.map(sanitiseGenericNames);

  function sanitiseGenericNames(content: string | null | undefined) {
    if (!content) return content;
    return genericNames
      ? genericNames.reduce((prev, cur) => {
          return replaceComponentNameUsage(cur, prev);
        }, content)
      : content;
  }

  const CompGeneric = genericDeclaration.items
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

  const InstanceGeneric = genericDeclaration.items
    .map((x) => {
      const name = x.name;
      const constraint = x.constraint || getGenericComponentName(x.name);
      const defaultType = x.default || getGenericComponentName(x.name);

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
    /**
     * this is to be used for the external component
     */
    component: CompGeneric,
    /**
     * This is to be a direct replace from user declaration
     */
    instance: InstanceGeneric,

    genericNames,
    sanitisedNames,
  };
}

export function processOptions(context: ParseScriptContext) {
  // todo move locations to argument
  const locations = parseLocations(context);
  const filename = getBlockFilename("options", context);

  if (!locations.import) {
    locations.import = [];
  }

  const s = context.s.clone();

  // remove unknown blocks
  const SUPPORTED_BLOCKS = new Set(["script"]);
  const blocks: VerterSFCBlock[] = [];
  for (const block of context.blocks) {
    if (SUPPORTED_BLOCKS.has(block.tag.type)) {
      blocks.push(block);
    } else {
      s.remove(block.tag.pos.open.start, block.tag.pos.close.end);
    }
  }

  if (context.script) {
    const isSetup = context.isSetup;
    const isTypescript = context.script.lang?.startsWith("ts") ?? false;
    const generic = context.generic ? genericProcess(context, locations) : null;

    const mainBlock =
      blocks.find(
        (x) => x.tag.type === "script" && x.block.setup === isSetup
      ) ?? blocks.find((x) => x.tag.type === "script");

    const scriptEndOffset = mainBlock.tag.pos.close.end;
    const startOffset = context.script.loc.start.offset;

    // do work for <script ... >
    if (isSetup) {
      const preGeneric = `\nexport ${
        context.isAsync ? "async " : ""
      }function Build${BindingContextExportName}`;
      const postGeneric = `() {\n`;

      // remove close tag
      s.remove(mainBlock.tag.pos.close.start, mainBlock.tag.pos.close.end);

      const generic = context.generic;
      // generic information will be kept intact for the source-map
      if (typeof generic === "string") {
        // get <script ... >
        const tagContent = s.original.slice(
          mainBlock.tag.pos.open.start,
          mainBlock.tag.pos.open.end
        );

        const genericIndex =
          tagContent.indexOf(generic) + mainBlock.tag.pos.open.start;

        // replace before generic with `preGeneric`
        s.overwrite(
          mainBlock.tag.pos.open.start,
          genericIndex,
          preGeneric + "<"
        );

        // replace after generic with `postGeneric`
        s.overwrite(
          genericIndex + generic.length,
          context.script.loc.start.offset,
          ">" + postGeneric
        );
      } else {
        s.overwrite(
          mainBlock.tag.pos.open.start,
          mainBlock.tag.pos.open.end,
          preGeneric + postGeneric
        );
      }

      // s.append('\n}\n')

      // context.s.overwrite(
      //     block.tag.pos.close.start,
      //     block.tag.pos.close.end,
      //     "\n}\n"
      // );
    } else {
      // todo
    }

    // remove block tags
    blocks.forEach((block) => {
      // check if the block was already handled
      if (block === mainBlock && isSetup) return;
      s.remove(block.tag.pos.open.start, block.tag.pos.open.end);

      s.remove(block.tag.pos.close.start, block.tag.pos.close.end);
    });

    // context function
    if (isSetup) {
      const bindings = Object.keys(context.script.bindings ?? {});
      const typeofBindings = bindings
        .map((x) => `${x}: typeof ${x}`)
        .join(", ");
      if (isTypescript) {
        s.appendRight(
          scriptEndOffset,
          `\nreturn {} as {${typeofBindings}\n}\n`
        );
      } else {
        // if not typescript we need to the types in JSDoc to make sure the types are correct
        // because when the variables are const the types are not inferred correctly
        // e.g. const a = 1; a is inferred as number but it should be inferred as 1
        s.prependRight(
          mainBlock.tag.pos.open.start,
          `/**\n * @returns {{${typeofBindings}}} \n*/`
        );

        s.appendRight(
          scriptEndOffset,
          `\nreturn {\n${bindings.join(", ")}\n}\n`
        );
      }

      // close the BuildBindingContext function
      s.appendRight(scriptEndOffset, `\n}\n`);

      // TODO should be moved to the render and bundler
      // because this file can be javascript
      //   s.append(
      //     `\nexport type ${BindingContextExportName}${
      //       context.generic ? `<${context.generic}>` : ""
      //     } = ReturnType<typeof Build${BindingContextExportName}${
      //       generic ? `<${generic.genericNames.join(",")}>` : ""
      //     }>${isAsync ? ` extends Promise<infer R> ? R : never` : ""};\n`
      //   );
    }

    // expose the compile script
    let compiled = "\n" + context.script.content;
    {
      // export
      compiled = compiled.replace(
        "const ____VERTER_COMP_OPTION__",
        `const ____VERTER_COMP_OPTION__COMPILED`
      );
      // remove imports and move imports to the top
      if (locations.import.length > 0) {
        locations.import.forEach((x) => {
          if (!x.node) return;
          const start = x.node.start + x.offset;
          const end = x.node.end + x.offset;
          const content = s.original.slice(start, end);
          compiled = compiled.replace(content, "");

          s.move(start, end, 0);
        });
      }

      const toRemoveLocations = [
        ...(locations.declaration ?? []),
        ...(locations.export ?? []),
      ];

      if (toRemoveLocations.length > 0) {
        toRemoveLocations.forEach((x) => {
          if (!x.node) return;
          const script =
            x.isSetup === false
              ? context.sfc.descriptor.script
              : context.sfc.descriptor.scriptSetup ??
                context.sfc.descriptor.script;

          const content = script.loc.source.slice(x.node.start, x.node.end);
          compiled = compiled.replace(content, "");
        });
      }

      locations.import.push({
        type: LocationType.Import,
        generated: true,
        from: "vue",
        items: [
          {
            name: "defineComponent",
            alias: "__VERTER__defineComponent",
          },
        ],
      });

      if (isTypescript) {
        const CHECK_DEFINE_COMPONENT = `declare function __VERTER__isDefineComponent<T>(o: T): o is (T extends __VERTER__DefineComponent<any, any, any, any, any> ? T : T & never);`;

        locations.import.push({
          type: LocationType.Import,
          generated: true,
          from: "vue",
          items: [
            {
              name: "DefineComponent",
              alias: "__VERTER__DefineComponent",
              type: true,
            },
          ],
        });

        // check if we should mark as const
        if (compiled.endsWith("}")) {
          // NOTE casting as const might cause some unintended side effects
          // we need to keep track to see if there's no bad side effects
          compiled += " as const;";
        }

        // TODO should import defineComponent and DefineComponent
        // TODO rename to prefix __VERTER__
        compiled += [
          "",
          CHECK_DEFINE_COMPONENT,
          `const ____VERTER_COMP_OPTION__RESULT = __VERTER__isDefineComponent(____VERTER_COMP_OPTION__COMPILED) ? ____VERTER_COMP_OPTION__COMPILED : __VERTER__defineComponent(____VERTER_COMP_OPTION__COMPILED)`,
          "export const ____VERTER_COMP_OPTION__ = {} as typeof ____VERTER_COMP_OPTION__COMPILED & typeof ____VERTER_COMP_OPTION__RESULT;",
        ].join("\n");
      } else {
        compiled += [
          "",
          "export const ____VERTER_COMP_OPTION__ = __VERTER__defineComponent(____VERTER_COMP_OPTION__COMPILED);",
        ].join("\n");
      }
    }

    {
      // add generated imports
      if (locations.import?.length > 0) {
        const from: Record<string, Set<string>> = {};

        locations.import
          .filter((x) => x.generated)
          .forEach((it) => {
            if (!from[it.from]) {
              from[it.from] = new Set();
            }
            const importItem = from[it.from];
            for (let i = 0; i < it.items.length; i++) {
              const item = it.items[i];
              importItem.add(
                `${it.asType || item.type ? "type " : ""}${item.name}${
                  item.alias ? ` as ${item.alias}` : ""
                }`
              );
            }
          });

        for (const [fromStr, imports] of Object.entries(from)) {
          s.prepend(
            `import { ${[...imports].join(", ")} } from '${fromStr}';\n`
          );
        }
      }
    }

    // remove default export if exists
    {
      locations.export
        ?.filter((x) => x.item.default)
        .forEach((x) => {
          if (!x.node) return;
          s.remove(x.node.start + x.offset, x.node.end + x.offset);
        });
    }

    s.append(compiled);
  }

  return {
    filename,
    blocks,
    loc: {
      source: s.original.toString(),
    },
    // loc: context.script?.loc ?? { source: "" },

    s,
    content: s.toString(),
  };
}
