import { checkForSetupMethodCall, retrieveNodeString } from "../helpers.js";
import { LocationType, PluginOption } from "../types.js";

export default {
  name: "Emits",

  walk: (node, context) => {
    if (!context.isSetup) return;
    const expression = checkForSetupMethodCall("defineEmits", node);
    if (!expression) return;
    const source = context.script!.loc.source;

    return [
      {
        type: LocationType.Import,
        node: expression,
        generated: true,
        // TODO change the import location
        from: "vue",
        items: [
          {
            name: "DeclareEmits",
            type: true,
          },
        ],
      },
      // create variable with return
      {
        type: LocationType.Declaration,
        node: expression,

        generated: true,
        declaration: {
          name: "__emits",
          content: retrieveNodeString(expression, source) || "{}",
        },
      },
      // get the type from variable
      {
        type: LocationType.Declaration,
        node: expression,
        generated: true,

        // TODO debug this to check if this is the correct type
        declaration: {
          type: "type",
          name: "Type__emits",
          content: `DeclareEmits<typeof __emits>;`,
        },
      },
      {
        type: LocationType.Emits,
        node: expression,
        content: "Type__emits",
      },
    ];
  },
} satisfies PluginOption;
