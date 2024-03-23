import { LocationByType, LocationType, ParseScriptContext, TypeLocationDeclaration } from "../plugins";
import { VerterSFCBlock, extractBlocksFromDescriptor, retrieveHTMLComments } from "../utils/sfc";


const possibleExports = [
    "export default /*#__PURE__*/_",
    "export default /*#__PURE__*/",
    "export default ",
];




export function mergeFull(locations: LocationByType, context: ParseScriptContext) {
    sanitiseMap(locations)

    const { s, generic, isSetup, sfc } = context;

    // const templateStartOffset = context.sfc.descriptor.template.loc.start.offset;
    const templateStartOffset = context.s.original.indexOf('<template')
    const templateEndOffset = context.s.original.indexOf('</template>')


    const allBlocks = extractBlocksFromDescriptor(sfc.descriptor);
    const blocks: VerterSFCBlock[] = []
    const SUPPORTED_BLOCKS = new Set(['template', 'script'])

    const source = sfc.descriptor.source


    {
        const allHTMLComments = retrieveHTMLComments(s.original)
        // remove comments on blocks
        // const comments = retrieveHTMLComments(s.original).filter((({ start, end }) => {
        //     // remove comments inside other blocks
        //     const anyIntersection = allBlocks.filter(b => b.tag.pos.open.start >= start && end <= b.tag.pos.close.end)
        //     return anyIntersection.length === 0
        // }))

        const comments = allHTMLComments.filter(({ start, end }) =>
            !allBlocks.some(b => b.tag.pos.open.start <= end && start <= b.tag.pos.close.end)
        );

        comments.forEach(({ start, end }) => s.remove(start, end))
    }


    // remove unknown blocks
    for (const block of allBlocks) {
        if (SUPPORTED_BLOCKS.has(block.block.type)) {
            blocks.push(block)
        } else {
            s.remove(block.tag.pos.open.start, block.tag.pos.close.end)
        }
    }

    {
        // move template to the bottom
        const templateIndex = blocks.findIndex(x => x.block.type === 'template')
        if (templateIndex >= 0 && templateIndex < blocks.findIndex(x => x.block.type === 'script')) {
            const template = blocks[templateIndex]
            s.move(template.tag.pos.open.start, template.tag.pos.close.end, s.original.length)
        }
    }





    // if is setup, there's a change to have 2 script tags
    // we need to bring the non-setup to the top
    if (isSetup) {
        const scriptBlocks = blocks.filter(x => x.block.type === 'script')
        if (scriptBlocks.length > 1) {
            const firstBlock = scriptBlocks[0]
            if (firstBlock.block.attrs.setup === true) {
                s.move(firstBlock.tag.pos.open.start, firstBlock.tag.pos.close.end, 0)
            }
        }

    }






    if (context.sfc.descriptor.scriptSetup) {
        const scriptSource = context.sfc.descriptor.scriptSetup.content;

        const scriptStartOffset = context.sfc.descriptor.scriptSetup.loc.start.offset;
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

                let declarationStr = 'const ____VERTER_COMP_OPTION__ = ';
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

                    s.replace(possibleExport, declarationStr)
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
            const regexComp = /const ____VERTER_COMP_OPTION__ = [^\w]*{]*/gm

            const rawObjectDeclaration = regexComp.test(s.toString());


            if (!rawObjectDeclaration) {
                locations[LocationType.Import].push({
                    type: LocationType.Import,
                    from: 'vue', node: null,
                    items: [
                        {
                            name: 'defineComponent',
                            alias: '___VERTER_defineComponent',
                            // note could be type
                            type: false
                        }
                    ]
                })
            }

            s.appendLeft(scriptEndOffset, `const ___VERTER_COMP___ = ${rawObjectDeclaration ? `___VERTER_defineComponent(___VERTER_COMP_OPTION__)` : '___VERTER_COMP_OPTION__'} `)
        }
    }


    // process template 

    // override <template> to function ___VERTER__TEMPLATE_RENDER()


    if (context.template) {
        s.overwrite(templateStartOffset, context.template.loc.start.offset, `function ___VERTER__TEMPLATE_RENDER${generic ? `<${generic}>` : ''}() {\n<>`)

        s.overwrite(templateEndOffset, templateEndOffset + '</template>'.length, '\n</>\n}')


        s.appendRight(templateStartOffset, '\n\n/* verter:template-render start */\n')
        s.appendRight(templateEndOffset + '</template>'.length, '\n/* verter:template-render end */\n')
    }

    // /process template


    // append tsx 

    s.prepend(`/* @jsxImportSource vue */\n`)

    // /append tsx


    // const scriptSource = context.script.loc.source;
    const nonGeneratedDeclarations = locations[LocationType.Declaration].filter(x => !x.generated)
    const imports = locations[LocationType.Import];


    // Get all declared names in the setup
    const declaredNames = new Set(nonGeneratedDeclarations
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
        .filter(Boolean))





    // generate ctx variable 

    const returnVars: string[] = [];

    if (isSetup) {
        returnVars.push(...imports.filter(x => !x.generated).flatMap(x => x.items.map(i => i.alias ?? i.name)))
    }
    // TODO add generic support
    returnVars.push('...(new ____VERTER_COMP_OPTION__())');

    returnVars.push(...Array.from(declaredNames).map(x => [x, `unref(${x})`].join(": ")))


    const ctx = `const ___VERTER_ctx = {\n${returnVars.join('\n')}\n}`;


    s.appendLeft(templateStartOffset, '\n/* verter:template-context start */\n' + ctx + '\n/* verter:template-context end */\n')
    // /generate ctx variable


    // generate comp variable

    // const comp = 

    // /generate comp variable


    // imports


    // /imports




    return {
        locations,
        source: context.s.original,
        content: context.s.toString(),
        map: context.s.generateMap({ hires: true, includeContent: true }),

        context
    }
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