import {
    MagicString,
    SFCScriptCompileOptions,
    parse,
} from "@vue/compiler-sfc";
import { defaultPlugins, ParseScriptContext, PluginOption } from "./plugins";
import { pluginsToLocations } from "./utils/plugin";
export function createContext(source: string, filename: string = 'temp.vue', options: Partial<SFCScriptCompileOptions> = {}) {
    // add empty script at the end
    if (
        source.indexOf("</script>") === -1 &&
        source.indexOf("<script ") === -1 &&
        source.indexOf("<script>") === -1
    ) {
        source += `\n<script></script>\n`;
    }


    const parsed = parse(source, {
        ...options,
        filename,
        sourceMap: true,
        ignoreEmpty: false,
    });

    const script = parsed.descriptor.scriptSetup ?? parsed.descriptor.script;

    const context = {
        id: filename,
        filename,
        isSetup: !!parsed.descriptor.scriptSetup,
        isAsync: false,
        sfc: parsed,
        script,
        generic: script.attrs.generic as string,
        template: parsed.descriptor.template,
        s: new MagicString(source),
    } satisfies ParseScriptContext;

    return context
}

export function parseLocations(context: ParseScriptContext, appendPlugins: PluginOption[] = []) {
    const plugins = [
        ...defaultPlugins,
        ...appendPlugins
    ]
    const locations = pluginsToLocations(plugins, context);
    return locations;
}