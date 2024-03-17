import { checkForSetupMethodCall, retrieveNodeString } from "../../helpers.js";
import { LocationType, PluginOption } from "../../types.js";

export default {
  name: "Slots",

  walk: (node, context) => {
    if (!context.isSetup) return;
    const expression = checkForSetupMethodCall("defineSlots", node);
    if (!expression) return;
    const source = context.script!.loc.source;

    const slotType = expression.typeParameters?.params[0];
    if (!slotType) return;

    return [
      {
        type: LocationType.Import,
        generated: true,
        node: expression,
        // TODO change the import location
        from: "vue",
        items: [
          {
            name: "SlotsType",
            type: true,
          },
        ],
      },
      {
        type: LocationType.Slots,
        node: expression,
        content: `SlotsType<${retrieveNodeString(slotType, source)}>`,
      },
    ];
  },
} satisfies PluginOption;
