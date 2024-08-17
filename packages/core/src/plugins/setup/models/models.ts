import { checkForSetupMethodCall, retrieveNodeString } from "../../helpers.js";
import { LocationType, PluginOption } from "../../types.js";

export default {
  name: "Model",

  walk: (node, context) => {
    if (!context.isSetup) return;

    const expression = checkForSetupMethodCall("defineModel", node);
    if (!expression) return;

    const source = context.script!.loc.source;
    const [nameOrOptions, options] = expression.arguments;

    const varName =
      node.type === "VariableDeclaration" &&
      node.declarations.length === 1 &&
      "name" in node.declarations[0].id
        ? node.declarations[0].id.name
        : undefined;

    const hasNamedArgument = nameOrOptions?.type === "StringLiteral";
    const modelName = (hasNamedArgument && nameOrOptions.value) || "modelValue";

    return [
      {
        type: LocationType.Model,
        generated: false,
        expression,
        content: retrieveNodeString(expression, source),
        varName,
        modelName,
      },
    ];
  },
} satisfies PluginOption;
