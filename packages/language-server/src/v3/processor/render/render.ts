import { LocationByType, ParseScriptContext, TemplateBuilder } from "@verter/core";


export function processRender(context: ParseScriptContext, locations: LocationByType) {
    const s = context.s.snip(context.template.loc.start.offset, context.template.loc.end.offset)


    return {
        filename: context.filename + '.render.tsx',
        source: s.toString(),
        s,

        context,
        locations
    }
}