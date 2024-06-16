// it should generate the script 
import { type ParseScriptContext } from "@verter/core";
import { parseLocations } from "@verter/core";
import { getBlockFilename } from "../utils";
import { MagicString } from "vue/compiler-sfc";

export const RenderContextExportName = 'RenderContext'

export function processScript(context: ParseScriptContext) {
    const locations = parseLocations(context);
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
        s = context.s.snip(context.script.loc.start.offset, context.script.loc.end.offset)

        // TODO sort this out
        const prependContent = [
            `export function ${RenderContextExportName}${generic}() {`,
            'return {',
            ''
        ]

        const appendContent = [
            '',
            '}',
            '}',
            ''
        ]

        s.prepend(prependContent.join('\n'))
        s.append(appendContent.join('\n'))
    }





    return {
        filename,
        loc: context.script?.loc ?? {},

        s,
        content: s.toString(),
    }

}