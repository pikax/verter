import { checkForSetupMethodCall, retrieveNodeString } from "../../helpers.js";
import { LocationType, PluginOption } from "../../types.js";

export default {
  name: "Slots",

  walk: (node, context) => {
    if (!context.isSetup) return;
    const expression = checkForSetupMethodCall("defineSlots", node);
    if (!expression) return;
    const source = context.script!.loc.source;

    const varName =
      node.type === "VariableDeclaration" &&
      node.declarations.length === 1 &&
      "name" in node.declarations[0].id
        ? node.declarations[0].id.name
        : undefined;

    if (
      context.generic &&
      (expression.typeParameters?.params.length ?? 0) > 0
    ) {
      return [
        // {
        //   type: LocationType.Import,
        //   node: expression,
        //   // TODO change the import location
        //   from: "vue",
        //   items: [
        //     {
        //       name: "ExtractPropTypes",
        //       type: true,
        //     },
        //   ],
        // },

        {
          type: LocationType.Slots,
          generated: false,
          node: expression.typeParameters!.params[0],
          content: retrieveNodeString(
            expression.typeParameters!.params[0],
            source
          ),
          expression,
          varName,
        },
      ];
    }

    return [
      {
        type: LocationType.Slots,
        generated: false,
        content: retrieveNodeString(expression, source),
        expression,
        varName,
      },

      // todo maybe add the type and what not
    ];

    // const slotType = expression.typeParameters?.params[0];
    // if (!slotType) return;

    // return [
    //   {
    //     type: LocationType.Import,
    //     generated: true,
    //     node: expression,
    //     // TODO change the import location
    //     from: "vue",
    //     items: [
    //       {
    //         name: "SlotsType",
    //         type: true,
    //       },
    //     ],
    //   },
    //   {
    //     type: LocationType.Slots,
    //     node: expression,
    //     content: `SlotsType<${retrieveNodeString(slotType, source)}>`,
    //   },
    // ];
  },
} satisfies PluginOption;
