import {
  LocationByType,
  LocationType,
  parseLocations,
  ParseScriptContext,
} from "@verter/core";
import { getBlockFilename } from "../utils";
import { MagicString } from "vue/compiler-sfc";
import { transform } from "sucrase";

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

  let s: MagicString;
  if (context.script == null) {
    s = new MagicString("");
  } else {
    const scriptStartOffset = context.s.original
      .slice(0, context.script.loc.start.offset)
      .lastIndexOf("<script");
    const startOffset = context.script.loc.start.offset;
    const endOffset = context.script.loc.end.offset;

    s = context.s.snip(scriptStartOffset, endOffset);
    const isAsync = context.isAsync;
    const isTypescript = context.script.lang?.startsWith("ts") ?? false;
    const generic = context.generic ? genericProcess(context, locations) : null;

    // do work for <script ... >
    {
      const preGeneric = `\nexport ${
        context.isAsync ? "async " : ""
      }function Build${BindingContextExportName}`;
      const postGeneric = `() {\n`;

      const generic = context.generic;
      // generic information will be kept intact for the source-map
      if (typeof generic === "string") {
        // get <script ... >
        const tagContent = context.s.original.slice(
          scriptStartOffset,
          context.script.loc.start.offset
        );

        const genericIndex = tagContent.indexOf(generic);

        // replace before generic with `preGeneric`
        s.overwrite(0, genericIndex, preGeneric + "<");

        // replace after generic with `postGeneric`
        s.overwrite(
          genericIndex + generic.length,
          context.script.loc.start.offset,
          ">" + postGeneric
        );
      } else {
        s.overwrite(
          0,
          context.script.loc.start.offset,
          preGeneric + postGeneric
        );
      }

      // s.append('\n}\n')

      // context.s.overwrite(
      //     block.tag.pos.close.start,
      //     block.tag.pos.close.end,
      //     "\n}\n"
      // );
    }

    /**
     * This will move the import statements to the top of the file
     * and also keep track of the end of the import statements
     */
    let importContentEndIndex = 0;

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
          const start = x.node.start + startOffset;
          const end = x.node.end + startOffset;
          const content = context.s.original.slice(start, end);
          compiled = compiled.replace(content, "");

          s.move(start, end, 0);

          if (importContentEndIndex < end) {
            importContentEndIndex = end;
          }
        });
      }

      if (locations.export?.length > 0) {
        // remove default export
        locations.export.forEach((x) => {
          if (!x.node) return;
          const content = context.s.slice(
            x.node.start + startOffset,
            x.node.end + startOffset
          );
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

    // context function
    if (context.isSetup) {
      // s.prependLeft(importContentEndIndex, `\n${isAsync ? 'async ' : ''}function Build${BindingContextExportName}${generic}() {\n`)

      if (isTypescript) {
        s.append(
          `\nreturn {} as {${Object.keys(context.script.bindings ?? {})
            .map((x) => `${x}: typeof ${x}`)
            .join(", ")}\n}\n`
        );
      } else {

        s.append(
            `\nreturn {\n${Object.keys(context.script.bindings ?? {})
              .join(", ")}\n}\n`
          );
      }
      s.append(`\n}\n`);

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
    s.append(compiled);
  }

  return {
    filename,
    loc: context.script?.loc ?? { source: "" },

    s,
    content: s.toString(),
  };
}
