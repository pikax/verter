import { checkForSetupMethodCall, retrieveNodeString } from "../../helpers.js";
import { LocationType, PluginOption } from "../../types.js";

export default {
  name: "Props",

  walk: (node, context) => {
    if (!context.isSetup) return;
    const expression =
      checkForSetupMethodCall("withDefaults", node) ||
      checkForSetupMethodCall("defineProps", node);
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
          type: LocationType.Props,
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
        type: LocationType.Props,
        generated: false,
        content: retrieveNodeString(expression, source),
        expression,
        varName,
      },

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
      // create variable with return
      // {
      //   type: LocationType.Declaration,
      //   node: expression,
      //   expression,

      //   generated: true,

      //   declaration: {
      //     name: "__props",

      //     // content: expression.arguments.length.

      //     content: retrieveNodeString(expression, source) || "{}",
      //     // content: expression.typeParameters?.params.length
      //     //   ? retrieveNodeString(expression, source)
      //     //   : expression.arguments.length === 0
      //     //   ? "defineProps({})"
      //     //   : // Simple method to extract the correct prop types, this will keep
      //     //     // required: true, instead of `required: boolean`, const cast could also
      //     //     // be possible, but it would change the type of Component.props
      //     //     `(<T extends Record<string, Prop<any>>>(o: T) => o) ({ ${
      //     //       retrieveNodeString(expression, source) || "{}"
      //     //     } })`,
      //   },
      // },
      // // get the type from variable
      // {
      //   type: LocationType.Declaration,
      //   node: expression,
      //   expression,

      //   generated: true,

      //   declaration: {
      //     type: "type",
      //     name: "Type__props",
      //     content: `typeof __props;`,
      //   },
      // },
      // {
      //   type: LocationType.Props,
      //   node: expression,
      //   content: "Type__props",
      // },
    ];
  },

  // process(locations, context) {
  //   if (
  //     !locations[LocationType.Props] ||
  //     locations[LocationType.Props].length === 0
  //   )
  //     return;
  //   const props = locations[LocationType.Props];
  // },
} satisfies PluginOption;
