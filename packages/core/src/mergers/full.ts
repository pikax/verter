import { MagicString } from "@vue/compiler-sfc";
import {
  ImportItem,
  LocationByType,
  LocationType,
  ParseScriptContext,
  TypeLocationDeclaration,
} from "../plugins";
import {
  VerterSFCBlock,
  extractBlocksFromDescriptor,
  retrieveHTMLComments,
} from "../utils/sfc";

const possibleExports = [
  "export default /*#__PURE__*/_",
  "export default /*#__PURE__*/",
  "export default ",
];

export function mergeFull(
  locations: LocationByType,
  context: ParseScriptContext
) {
  sanitiseMap(locations);

  const { s, generic, isSetup, sfc } = context;
  const source = s.original;

  const allBlocks = extractBlocksFromDescriptor(sfc.descriptor);
  const blocks: VerterSFCBlock[] = [];
  const SUPPORTED_BLOCKS = new Set(["template", "script"]);

  const startScript = sfc.descriptor.scriptSetup ?? sfc.descriptor.script;

  const startScriptIndex = startScript?.loc.start.offset;
  const endScriptIndex = startScript?.loc.end.offset;

  {
    const allHTMLComments = retrieveHTMLComments(source);

    const comments = allHTMLComments.filter(
      ({ start, end }) =>
        !allBlocks.some(
          (b) => b.tag.pos.open.start <= end && start <= b.tag.pos.close.end
        )
    );

    comments.forEach(({ start, end }) => s.remove(start, end));
  }

  // remove unknown blocks
  for (const block of allBlocks) {
    if (SUPPORTED_BLOCKS.has(block.tag.type)) {
      blocks.push(block);
    } else {
      s.remove(block.tag.pos.open.start, block.tag.pos.close.end);
    }
  }

  // if is setup, there's a change to have 2 script tags
  // we need to bring the non-setup to the top
  if (isSetup) {
    const scriptBlocks = blocks.filter((x) => x.block.type === "script");
    if (scriptBlocks.length > 1) {
      const firstBlock = scriptBlocks[0];
      if (firstBlock.block.attrs.setup === true) {
        s.move(firstBlock.tag.pos.open.start, firstBlock.tag.pos.close.end, 0);
      }
    }
  }

  {
    // move template to before </script>
    // this is better because we can use the component funciton generic
    // to fix some of the types
    const templateIndex = blocks.findIndex((x) => x.block.type === "template");
    if (templateIndex >= 0) {
      const templateBlock = blocks[templateIndex];

      s.move(
        templateBlock.tag.pos.open.start,
        templateBlock.tag.pos.close.end,
        endScriptIndex
      );
    }
  }

  for (const block of blocks) {
    processBlock(block, context, locations);
  }

  // append imports
  {
    // move non Generated imports to the top

    for (const { node, offset } of locations.import.filter(
      (x) => !x.generated
    )) {
      // node AST are offset based on the content not on the file
      s.move(offset + node.loc.start.index, offset + node.loc.end.index, 0);
    }

    // reduce generated imports
    const generated = locations.import
      .filter((x) => !!x.generated && !x.asType)
      .reduce((prev, cur) => {
        const arr = prev[cur.from] ?? (prev[cur.from] = []);
        arr.push(...cur.items);
        return prev;
      }, {} as Record<string, ImportItem[]>);

    // reduce generated type only imports
    const generatedTypes = locations.import
      .filter((x) => !!x.generated && x.asType)
      .reduce((prev, cur) => {
        const arr = prev[cur.from] ?? (prev[cur.from] = []);
        arr.push(...cur.items);
        return prev;
      }, {} as Record<string, ImportItem[]>);

    const importsGenerated = Object.entries(generated).map(
      ([fromKey, items]) =>
        `import { ${items
          .map((x) =>
            (x.type ? "type" : "") + x.alias
              ? `${x.name} as ${x.alias}`
              : x.name
          )
          .join(", ")} } from '${fromKey}';`
    );

    const importsGeneratedTyped = Object.entries(generatedTypes).map(
      ([fromKey, items]) =>
        `import type { ${items
          .map((x) => (x.alias ? `${x.name} as ${x.alias}` : x.name))
          .join(", ")} } from '${fromKey}';`
    );

    const importsStr = [...importsGenerated, ...importsGeneratedTyped].join(
      "\n"
    );
    if (importsStr.trim()) {
      s.appendLeft(0, importsStr + "\n");
    }
  }

  // adding some imports and variable context
  {
    const regexComp = /const ____VERTER_COMP_OPTION__ = [^\w]*{]*/gm;
    const rawObjectDeclaration = regexComp.test(s.toString());

    if (!rawObjectDeclaration) {
      locations[LocationType.Import].push({
        type: LocationType.Import,
        generated: true,
        from: "vue",
        node: null,
        items: [
          {
            name: "defineComponent",
            alias: "___VERTER_defineComponent",
            // note could be type
            type: false,
          },
        ],
      });
    }

    locations.declaration.push({
      type: LocationType.Declaration,
      generated: true,
      context: "post",
      declaration: {
        type: "const",
        name: "___VERTER__ctx",
        content: `{ ${["...(new ___VERTER_COMP___())"].join(",\n")} }`,
      },
    });

    locations.declaration.push({
      type: LocationType.Declaration,
      generated: true,
      context: "post",
      declaration: {
        type: "const",
        name: "___VERTER__comp",
        content: `{ 
            ...({} as ExtractRenderComponents<typeof ___VERTER__ctx>),
          }`,
      },
    });
  }

  // add the generated declarations
  {
    // s.prependLeft(endScriptIndex, "");

    // const pre = ;
    // "global" | "pre" | "post" | "end"

    function declarationToString(x: TypeLocationDeclaration): string {
      if (!x.declaration.name) {
        return x.declaration.content;
      }
      return [
        x.declaration.type ?? "const",
        x.declaration.name,
        "=",
        x.declaration.content,
      ].join(" ");
    }

    const generatedDeclarations = locations.declaration.filter(
      (x) => !!x.generated
    );

    const global = generatedDeclarations
      .filter((x) => x.context === "global" || !x.context)
      .map(declarationToString);

    const pre = generatedDeclarations
      .filter((x) => x.context === "pre")
      .map(declarationToString);

    const post = generatedDeclarations
      .filter((x) => x.context === "post")
      .map(declarationToString);

    const end = generatedDeclarations
      .filter((x) => x.context === "end")
      .map(declarationToString);

    // TODO add global, pre, end

    s.prependLeft(startScriptIndex, pre.join("\n") + "\n");

    s.prependLeft(endScriptIndex, post.join("\n") + "\n");
  }

  return {
    locations,
    source: context.s.original,
    content: context.s.toString(),
    map: context.s.generateMap({ hires: true, includeContent: true }),

    context,
  };

  // const templateStartOffset = context.sfc.descriptor.template.loc.start.offset;
  const templateStartOffset = source.indexOf("<template");
  const templateEndOffset = source.indexOf("</template>");

  if (context.sfc.descriptor.scriptSetup) {
    const scriptSource = context.sfc.descriptor.scriptSetup.content;

    const scriptStartOffset =
      context.sfc.descriptor.scriptSetup.loc.start.offset;
    const scriptEndOffset = context.sfc.descriptor.scriptSetup.loc.end.offset;

    // for (const it of s.original.matchAll(SCRIPT_START_TAG_CONTENT_REGEX)) {
    //     it.groups.content
    // }
  }
  if (context.sfc.descriptor.script) {
    const script = context.sfc.descriptor.script;

    // if (context.sfc.descriptor.scriptSetup) {
    //     // TODO handle merging of scripts
    //     throw new Error('not supported')
    // }

    const scriptStartOffset = context.sfc.descriptor.script.loc.start.offset;
    const scriptEndOffset = context.sfc.descriptor.script.loc.end.offset;

    // template is first
    if (scriptStartOffset > templateStartOffset && false) {
    } else {
      // script is first TODO NOTE THIS MIGHT REMOVE <Style or other blocks
      // remove <script>
      // s.remove(0, scriptStartOffset)

      // replace export default
      if (!context.isSetup) {
        // const original = s.original;

        let declarationStr = "const ____VERTER_COMP_OPTION__ = ";
        // const exportRegex = /^(const ____VERTER_COMP_OPTION__ =)[^\w{]*/gm

        // let exportIndex = -1

        for (const possibleExport of possibleExports) {
          //     exportIndex = original.indexOf(possibleExport);
          //     if (exportIndex >= 0) {
          //         let len = possibleExport.length;

          //     do {
          //         ++len
          //     } while (original.charCodeAt(len) < 33)

          //     if (original.charAt(len) === '{') {

          //     }

          //     s.overwrite(exportIndex, possibleExport.length)
          // }

          s.replace(possibleExport, declarationStr);
        }

        //     const generatedStr = s.toString()
        //     const startIndex = generatedStr.indexOf(declarationStr);
        //     if (startIndex > 0) {
        //         const sliced = generatedStr.slice(startIndex)
        //         const r = exportRegex.exec(sliced)
        //         // const matches = generatedStr.match(exportRegex);

        //         // if { add defineComponent
        //         if (sliced[r[0].length] === '{') {
        //             s.original

        //         }

        //         console.log('rrr', r, matches)
        //         // generatedStr.slice(index)
        //     }
      }

      // remove </script>
      // s.remove(scriptEndOffset, templateStartOffset)

      // check if there's defineComponent or is pure object
      const regexComp = /const ____VERTER_COMP_OPTION__ = [^\w]*{]*/gm;

      const rawObjectDeclaration = regexComp.test(s.toString());

      if (!rawObjectDeclaration) {
        locations[LocationType.Import].push({
          type: LocationType.Import,
          from: "vue",
          node: null,
          items: [
            {
              name: "defineComponent",
              alias: "___VERTER_defineComponent",
              // note could be type
              type: false,
            },
          ],
        });
      }

      s.appendLeft(
        scriptEndOffset,
        `const ___VERTER_COMP___ = ${
          rawObjectDeclaration
            ? `___VERTER_defineComponent(___VERTER_COMP_OPTION__)`
            : "___VERTER_COMP_OPTION__"
        } `
      );
    }
  }

  // append tsx

  s.prepend(`/* @jsxImportSource vue */\n`);

  // /append tsx

  // const scriptSource = context.script.loc.source;
  const nonGeneratedDeclarations = locations[LocationType.Declaration].filter(
    (x) => !x.generated
  );
  const imports = locations[LocationType.Import];

  // Get all declared names in the setup
  const declaredNames = new Set(
    nonGeneratedDeclarations
      .map((x) => {
        switch (x.node.type) {
          case "VariableDeclaration": {
            return x.node.declarations
              .map((x) => {
                // TODO handle withdefaults too
                if (x.init?.type === "CallExpression") {
                  // TODO handle this case
                  // if (x.init.callee.name === "defineProps") {
                  //     propsName = x.id.name;
                  //     // TODO add argument support
                  //     if (x.init.arguments) {
                  //     }
                  //     x.init.typeParameters?.params?.[0]?.members?.forEach(
                  //         (x) => propsProps.add(x.key.name)
                  //     );
                  // }
                } else if (x.id.type === "ObjectPattern") {
                  return x.id.properties.map((x) => x.key.name).join(", ");
                }
                return x.id.name;
              })
              .filter(Boolean)
              .join(", ");
          }
          case "FunctionDeclaration": {
            return x.node.id.name;
          }
          case "EnumDeclaration": {
            return x.node.id.name;
          }
          case "ClassDeclaration": {
            return x.node.id.name;
          }
          default: {
            return "";
          }
        }
      })
      .filter(Boolean)
  );

  // generate ctx variable

  const returnVars: string[] = [];

  if (isSetup) {
    returnVars.push(
      ...imports
        .filter((x) => !x.generated)
        .flatMap((x) => x.items.map((i) => i.alias ?? i.name))
    );
  }
  // TODO add generic support
  returnVars.push("...(new ____VERTER_COMP_OPTION__())");

  returnVars.push(
    ...Array.from(declaredNames).map((x) => [x, `unref(${x})`].join(": "))
  );

  const ctx = `const ___VERTER_ctx = {\n${returnVars.join("\n")}\n}`;

  s.appendLeft(
    templateStartOffset,
    "\n/* verter:template-context start */\n" +
      ctx +
      "\n/* verter:template-context end */\n"
  );
  // /generate ctx variable

  // generate comp variable

  // /generate comp variable

  // imports

  // /imports

  return {
    locations,
    source: context.s.original,
    content: context.s.toString(),
    map: context.s.generateMap({ hires: true, includeContent: true }),

    context,
  };
}

// make sure all the locations are valid
function sanitiseMap(map: LocationByType) {
  map[LocationType.Props] ?? (map[LocationType.Props] = []);
  map[LocationType.Emits] ?? (map[LocationType.Emits] = []);
  map[LocationType.Slots] ?? (map[LocationType.Slots] = []);
  map[LocationType.Options] ?? (map[LocationType.Options] = []);
  map[LocationType.Model] ?? (map[LocationType.Model] = []);
  map[LocationType.Expose] ?? (map[LocationType.Expose] = []);
  map[LocationType.Template] ?? (map[LocationType.Template] = []);
  map[LocationType.Generic] ?? (map[LocationType.Generic] = []);
  map[LocationType.Declaration] ?? (map[LocationType.Declaration] = []);
  map[LocationType.Import] ?? (map[LocationType.Import] = []);
  map[LocationType.Export] ?? (map[LocationType.Export] = []);
}

function processBlock(
  block: VerterSFCBlock,
  context: ParseScriptContext,
  locations: LocationByType
) {
  const { s } = context;
  switch (block.tag.type) {
    case "script": {
      scriptTagProcess(block, context);
      const declarationStr = "const ____VERTER_COMP_OPTION__ = ";
      if (block.block.attrs.setup) {
        locations.import.push({
          type: LocationType.Import,
          from: "vue",
          generated: true,
          node: null,
          items: [
            {
              name: "defineComponent",
              alias: "___VERTER_defineComponent",
              // note could be type
              type: false,
            },
          ],
        });

        locations.declaration.push({
          type: LocationType.Declaration,
          generated: true,
          context: "pre",
          declaration: {
            // NOTE It might contain the other variables, we might need to slice
            content:
              context.script.content.replace(
                "____VERTER_COMP_OPTION__ = ",
                "____VERTER_COMP_OPTION__ = ___VERTER_defineComponent("
              ) + ")",
          },
        });

        // add ___VERTER_COMP___ in declaration
        locations.declaration.push({
          type: LocationType.Declaration,
          generated: true,
          context: "post",
          declaration: {
            type: "const",
            name: "___VERTER_COMP___",
            content: `____VERTER_COMP_OPTION__`,
          },
        });
      } else {
        // options
        {
          // override the export default
          for (const possibleExport of possibleExports) {
            s.replace(possibleExport, declarationStr);
          }

          if (s.toString().indexOf(declarationStr) === -1) {
            s.appendLeft(block.block.loc.end.offset, `${declarationStr}{};`);
          }

          // check if there's defineComponent or is pure object
          const regexComp = /const ____VERTER_COMP_OPTION__ = [^\w]*{]*/gm;

          const rawObjectDeclaration = regexComp.test(s.toString());

          if (rawObjectDeclaration) {
            locations.import.push({
              type: LocationType.Import,
              from: "vue",
              generated: true,
              node: null,
              items: [
                {
                  name: "defineComponent",
                  alias: "___VERTER_defineComponent",
                  // note could be type
                  type: false,
                },
              ],
            });
          }

          // add ___VERTER_COMP___ in declaration
          locations.declaration.push({
            type: LocationType.Declaration,
            generated: true,
            context: "post",
            declaration: {
              type: "const",
              name: "___VERTER_COMP___",
              content: rawObjectDeclaration
                ? `___VERTER_defineComponent(____VERTER_COMP_OPTION__)`
                : "____VERTER_COMP_OPTION__",
            },
          });
        }
      }

      return;
    }
    case "template": {
      // process template

      // override <template> to function ___VERTER__TEMPLATE_RENDER()
      s.overwrite(
        block.tag.pos.open.start,
        block.tag.pos.open.end,
        "function ___VERTER__TEMPLATE_RENDER() {\n<>"
      );

      // override </template>
      s.overwrite(block.tag.pos.close.start, block.tag.pos.close.end, "\n</>}");

      // if (context.template) {
      //     s.overwrite(templateStartOffset, context.template.loc.start.offset, `function ___VERTER__TEMPLATE_RENDER${generic ? `<${generic}>` : ''}() {\n<>`)

      //     s.overwrite(templateEndOffset, templateEndOffset + '</template>'.length, '\n</>\n}')

      //     s.appendRight(templateStartOffset, '\n\n/* verter:template-render start */\n')
      //     s.appendRight(templateEndOffset + '</template>'.length, '\n/* verter:template-render end */\n')
      // }

      // /process template

      return;
    }

    default: {
      throw new Error("Not recognised");
    }
  }
}

function clearTags(block: VerterSFCBlock, s: MagicString) {
  // clear opening tag
  s.remove(block.tag.pos.open.start, block.tag.pos.open.end);
  // clear closing tag
  s.remove(block.tag.pos.close.start, block.tag.pos.close.end);
}

function scriptTagProcess(block: VerterSFCBlock, context: ParseScriptContext) {
  const preGeneric = `\nfunction ___VERTER_COMPONENT__`;
  const postGeneric = `() {\n`;

  const generic =
    typeof context.generic === "string"
      ? context.generic
      : context.script?.attrs.generic;
  // generic information will be kept intact for the source-map
  if (typeof generic === "string") {
    const tagContent = context.s.original.slice(
      block.tag.pos.open.start,
      block.tag.pos.open.end
    );

    const genericIndex = tagContent.indexOf(generic);

    // replace before generic with `preGeneric`
    context.s.overwrite(
      block.tag.pos.open.start,
      block.tag.pos.open.start + genericIndex,
      preGeneric + "<"
    );

    // replace after generic with `postGeneric`
    context.s.overwrite(
      block.tag.pos.open.start + genericIndex + generic.length,
      block.tag.pos.open.end,
      ">" + postGeneric
    );
  } else {
    context.s.overwrite(
      block.tag.pos.open.start,
      block.tag.pos.open.end,
      preGeneric + postGeneric
    );
  }

  context.s.overwrite(
    block.tag.pos.close.start,
    block.tag.pos.close.end,
    "\n}\n"
  );
}
