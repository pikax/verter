import { LocationByType, LocationType, ParseScriptContext } from "../plugins";


export function mergeFull(locations: LocationByType, context: ParseScriptContext) {
    sanitiseMap(locations)

    const { s } = context;

    // const templateStartOffset = context.sfc.descriptor.template.loc.start.offset;
    const templateStartOffset = context.s.original.indexOf('<template')
    const templateEndOffset = context.s.original.indexOf('</template>')

    if (context.sfc.descriptor.scriptSetup) {
        const scriptSource = context.sfc.descriptor.scriptSetup.content;
    }
    if (context.sfc.descriptor.script) {
        const script = context.sfc.descriptor.script;

        if (context.sfc.descriptor.scriptSetup) {
            // TODO handle merging of scripts
            throw new Error('not supported')
        }

        const scriptStartOffset = context.sfc.descriptor.script.loc.start.offset;
        const scriptEndOffset = context.sfc.descriptor.script.loc.end.offset;


        // template is first
        if (scriptStartOffset > templateStartOffset) {


        } else {
            // script is first
            // remove <script>
            s.remove(0, scriptStartOffset)


            // replace export default
            s.replace('export default', `const ____VERTER_COMP_OPTION__ =`)


            // remove </script>
            s.remove(scriptEndOffset, templateStartOffset)
        }

    }


    // process template 

    // override <template> to function ___VERTER__TEMPLATE_RENDER()

    s.overwrite(templateStartOffset, context.template.loc.start.offset, 'function ___VERTER__TEMPLATE_RENDER() {')

    s.overwrite(templateEndOffset, templateEndOffset + '</template>'.length, '}')

    // /process template


    // append tsx 

    s.prepend(`/* @jsxImportSource vue */\nimport 'vue'\n`)
    // s.prepend(`/* @jsxImportSource vue */`)


    // const scriptSource = context.script.loc.source;
    const nonGenerated = locations[LocationType.Declaration].filter(x => !x.generated)



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