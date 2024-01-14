import { checkForSetupMethodCall, retrieveNodeString } from "../helpers.js";
import { LocationType, PluginOption } from "../types.js";

export default {
  name: "Props",

  walk: (node, context) => {
    if (!context.isSetup) return;
    const expression =
      checkForSetupMethodCall("withDefaults", node) ||
      checkForSetupMethodCall("defineProps", node);
    if (!expression) return;
    const source = context.script!.loc.source;

    return [
      {
        type: LocationType.Import,
        node: expression,
        items: [
          {
            name: "ExtractPropTypes",
            // TODO change the import location
            from: "<helpers>",
            type: true,
          },
        ],
      },
      // create variable with return
      {
        type: LocationType.Declaration,
        node: expression,

        declaration: {
          name: "__props",
          content: retrieveNodeString(expression, source) || "{}",
        },
      },
      // get the type from variable
      {
        type: LocationType.Declaration,
        node: expression,

        // TODO debug this to check if this is the correct type
        declaration: {
          type: "type",
          name: "Type__props",
          content: `ExtractPropTypes<typeof __props>;`,
        },
      },
      {
        type: LocationType.Props,
        node: expression,
        content: "Type__props",
      },
    ];
  },
} satisfies PluginOption;
