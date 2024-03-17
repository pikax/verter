import type { PluginOption } from "../types";

import EmitsPlugin from "./emits/index.js";
import PropsPlugin from "./props/index.js";
import SlotsPlugin from "./slots/index.js";
import ModelsPlugin from "./models/index.js";
import ExposePlugin from "./expose/index.js";


const DefaultPlugins = [
    EmitsPlugin,
    PropsPlugin,
    SlotsPlugin,
    ModelsPlugin,
    ExposePlugin,
] as Array<PluginOption>;

export default {
    name: 'Setup',

    walk(node, context) {
        if (!context.isSetup) return;
        return DefaultPlugins.flatMap(x => x.walk(node, context))
    }
} satisfies PluginOption