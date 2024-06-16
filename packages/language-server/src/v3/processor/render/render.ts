import { MagicString } from "vue/compiler-sfc";
import { type ParseScriptContext, TemplateBuilder, getAccessors } from "@verter/core";
import { relative } from 'path/posix'

import { RenderContextExportName } from '../script/index.js'
import { getBlockFilename } from "../utils.js";

export const FunctionExportName = 'Render'

export function processRender(context: ParseScriptContext) {
    const accessors = getAccessors()

    const filename = getBlockFilename("render", context)
    const scriptFilename = getBlockFilename("script", context)

    const relativeScriptPath = relative(context.filename, scriptFilename)
    const generic = context.generic ? `<${context.generic}>` : ''


    let s: MagicString

    if (context.template == null) {
        s = new MagicString([`export function ${FunctionExportName}() {} {`,
            `return <></>`,
            `} `
        ].join('\n')
        );
    } else {
        s = context.s.snip(context.template.loc.start.offset, context.template.loc.end.offset)

        const prependContent = [
            `import { ${RenderContextExportName} } from './${relativeScriptPath}'`,
            `export function ${FunctionExportName}${generic}() {`,
            `const ${accessors.ctx} = ${RenderContextExportName}${generic}()`,
            'return <>',
            ''
        ]

        const appendContent = [
            '',
            '</>',
            '}',
            ''
        ]
        TemplateBuilder.process({
            ...context,
            s
        })


        s.prepend(prependContent.join('\n'))
        s.append(appendContent.join('\n'))
    }

    return {
        filename,
        loc: context.template?.loc ?? {},
        context,
        s,
        content: s.toString(),
    }
}