export * from "./template.js";


import * as Plugins from './plugins/index.js';

export const DefaultPlugins = [
    Plugins.InterpolationPlugin,
    Plugins.PropPlugin,
    Plugins.BindingPlugin,
    Plugins.CommentPlugin,
    Plugins.TextPlugin,
    Plugins.SlotPlugin,
    Plugins.ConditionalPlugin,
    Plugins.BlockPlugin,
]