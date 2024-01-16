import { checkForSetupMethodCall, retrieveNodeString } from "../helpers.js";
import { LocationType, PluginOption } from "../types.js";

export default {
  name: "Props",

  walk: (node, context) => {
    if (!context.isSetup) return;
    const expression = checkForSetupMethodCall("defineExpose", node);
    if (!expression) return;

    // TODO add sourmap map
    return {
      type: LocationType.Expose,
      node: expression,

      content: retrieveNodeString(
        expression.arguments[0],
        context.script!.loc.source
      ),
    };
  },
} satisfies PluginOption;
