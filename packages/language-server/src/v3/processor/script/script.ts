// it should generate the script 
import { type ParseScriptContext } from "@verter/core";
import { parseLocations } from "@verter/core";
import { walkIdentifiers, extractIdentifiers } from '@vue/compiler-core'
import { getBlockFilename } from "../utils";
import { MagicString, type SFCScriptCompileOptions, type SFCDescriptor, compileScript } from "vue/compiler-sfc";

export const RenderContextExportName = 'RenderContext'


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


export function processScript(context: ParseScriptContext) {
    const compiled = compileScriptSafe(context.sfc.descriptor, {
        id: context.filename,
        genDefaultAs: "____VERTER_COMP_OPTION__",
        // ...config?.vue?.compiler,
        sourceMap: false,
        isProd: true,
    })

    const locations = parseLocations({
        ...context,
        script: compiled
    });
    const filename = getBlockFilename("script", context)

    const generic = context.generic ? `<${context.generic}>` : ''
    let s: MagicString

    // TODO add options function
    if (context.script == null) {
        s = new MagicString([`export function ${RenderContextExportName}${generic}() {} {`,
            `return {}`,
            `} `
        ].join('\n')
        );
    } else {
        const startOffset = context.script.loc.start.offset;
        const endOffset = context.script.loc.end.offset;

        s = context.s.snip(startOffset, endOffset)

        // TODO sort this out
        const prependContent = [
            '',
            `export function ${RenderContextExportName}${generic}() {`,
            ''
        ]

        const returnContent = [
            '',
            'return {'];

        const appendContent = [
            '',
            '}',
            '}',
            ''
        ]

        const ctxVariables = []



        /**
         * This will move the import statements to the top of the file
         * and also keep track of the end of the import statements 
         */
        let importContentEndIndex = 0
        if (locations.import?.length > 0) {
            for (const it of locations.import.filter(x => !x.generated)) {
                s.move(it.node.start + startOffset, it.node.end + startOffset, 0)
                if (importContentEndIndex < it.node.end + startOffset) {
                    importContentEndIndex = it.node.end + startOffset
                }

                if (it.node.specifiers.length > 0) {
                    // const imports = it.node.specifiers.map(x => x.local.name)
                    // ctxVariables.push(...imports)


                    walkIdentifiers(it.node, (name) => {
                        ctxVariables.push(name.name)
                    }, true)

                }

            }
        }

        if (locations.declaration?.length > 0) {
            locations.declaration.forEach(it => {
                // walkIdentifiers(it.node, (name) => {
                //     console.log('sss', name)
                //     debugger
                // }, true)

                walkIdentifiers(it.node, (name) => {
                    ctxVariables.push(name.name)
                }, true)
            })
        }


        const returnCtx = [...new Set(ctxVariables)].join(',\n')

        s.appendLeft(importContentEndIndex, prependContent.join('\n'))
        s.append(returnContent.concat(returnCtx).join('\n'))
        s.append(appendContent.join('\n'))
    }





    return {
        filename,
        loc: context.script?.loc ?? {},

        s,
        content: s.toString(),
    }

}