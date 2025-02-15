export * from "./template.js";

import * as Plugins from "./plugins/index.js";

export const DefaultPlugins = [
  Plugins.InterpolationPlugin,
  Plugins.PropPlugin,
  Plugins.BindingPlugin,
  Plugins.CommentPlugin,
  // Plugins.TextPlugin,
  Plugins.SlotPlugin,
  Plugins.BlockPlugin,
  Plugins.ConditionalPlugin,
  Plugins.EventPlugin,
  Plugins.LoopPlugin,
  Plugins.DirectivePlugin,
  Plugins.ElementPlugin,
  Plugins.TemplateTagPlugin,
  Plugins.SFCCleanerPlugin,
];

export const ScriptDefaultPlugins = [];
