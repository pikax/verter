import { LocationType, parseLocations, ParseScriptContext } from "@verter/core";
import { getBlockFilename } from "../utils";
import { MagicString } from "vue/compiler-sfc";
import { transform } from "sucrase";


export const OptionsExportName = 'ComponentOptions'

export const BindingContextExportName = 'BindingContext'

export function processOptions(context: ParseScriptContext) {
    // todo move locations to argument
    const locations = parseLocations(context);
    const filename = getBlockFilename("options", context)


    if (!locations.import) {
        locations.import = []
    }

    let s: MagicString
    if (context.script == null) {
        s = new MagicString('');
    } else {
        const startOffset = context.script.loc.start.offset;
        const endOffset = context.script.loc.end.offset;

        s = context.s.snip(startOffset, endOffset)
        const isAsync = context.isAsync
        const isTypescript = context.script.lang?.startsWith('ts') ?? false
        const generic = context.generic ? `<${context.generic}>` : ''

        /**
         * This will move the import statements to the top of the file
         * and also keep track of the end of the import statements 
         */
        let importContentEndIndex = 0


        // expose the compile script
        let compiled = '\n' + context.script.content;
        {
            // export
            compiled = compiled.replace('const ____VERTER_COMP_OPTION__', `const ____VERTER_COMP_OPTION__COMPILED`)
            // remove imports and move imports to the top
            if (locations.import.length > 0) {
                locations.import.forEach((x) => {
                    if (!x.node) return
                    const start = x.node.start + startOffset
                    const end = x.node.end + startOffset
                    const content = context.s.original.slice(start, end)
                    compiled = compiled.replace(content, '')

                    s.move(start, end, 0)

                    if (importContentEndIndex < end) {
                        importContentEndIndex = end
                    }

                })
            }

            if (locations.export?.length > 0) {
                // remove default export
                locations.export.forEach((x) => {
                    if (!x.node) return
                    const content = context.s.slice(x.node.start + startOffset, x.node.end + startOffset)
                    compiled = compiled.replace(content, '')
                })
            }

            locations.import.push({
                type: LocationType.Import,
                generated: true,
                from: 'vue',
                items: [
                    {
                        name: 'defineComponent',
                        alias: '__VERTER__defineComponent'
                    }
                ]
            })

            if (isTypescript) {
                const CHECK_DEFINE_COMPONENT = `declare function __VERTER__isDefineComponent<T>(o: T): o is (T extends __VERTER__DefineComponent<any, any, any, any, any> ? T : T & never);`

                locations.import.push({
                    type: LocationType.Import,
                    generated: true,
                    from: 'vue',
                    items: [
                        {
                            name: 'DefineComponent',
                            alias: '__VERTER__DefineComponent',
                            type: true
                        }
                    ]

                })

                // check if we should mark as const
                if (compiled.endsWith('}')) {
                    // NOTE casting as const might cause some unintended side effects
                    // we need to keep track to see if there's no bad side effects
                    compiled += ' as const;'
                }

                // TODO should import defineComponent and DefineComponent
                // TODO rename to prefix __VERTER__
                compiled += [
                    '',
                    CHECK_DEFINE_COMPONENT,
                    `const ____VERTER_COMP_OPTION__RESULT = __VERTER__isDefineComponent(____VERTER_COMP_OPTION__COMPILED) ? ____VERTER_COMP_OPTION__COMPILED : __VERTER__defineComponent(____VERTER_COMP_OPTION__COMPILED)`,
                    'export const ____VERTER_COMP_OPTION__ = {} as typeof ____VERTER_COMP_OPTION__COMPILED & typeof ____VERTER_COMP_OPTION__RESULT;'
                ].join('\n')

            } else {
                compiled += [
                    '',
                    'export const ____VERTER_COMP_OPTION__ = __VERTER__defineComponent(____VERTER_COMP_OPTION__COMPILED);',
                ].join('\n')
            }
        }

        // context function 
        if (context.isSetup) {
            s.prependLeft(importContentEndIndex, `\nexport ${isAsync ? 'async ' : ''}function ${BindingContextExportName}${generic}() {\n`)

            s.append(`\nreturn {} as {${Object.keys(context.script.bindings ?? {}).map(x => `${x}: typeof ${x}`).join(', ')}\n}\n`)
            s.append(`\n}\n`)
        }



        {
            // add generated imports
            if (locations.import?.length > 0) {
                const from: Record<string, Set<string>> = {}

                locations.import.filter(x => x.generated).forEach(it => {
                    if (!from[it.from]) {
                        from[it.from] = new Set()
                    }
                    const importItem = from[it.from]
                    for (let i = 0; i < it.items.length; i++) {
                        const item = it.items[i];
                        importItem.add(`${it.asType || item.type ? 'type ' : ''}${item.name}${item.alias ? ` as ${item.alias}` : ''}`)
                    }
                })

                for (const [fromStr, imports] of Object.entries(from)) {
                    s.prepend(`import { ${[...imports].join(', ')} } from '${fromStr}';\n`)
                }
            }
        }
        s.append(compiled)
    }


    return {
        filename,
        loc: context.script?.loc ?? { source: '' },

        s,
        content: s.toString(),
    }
}