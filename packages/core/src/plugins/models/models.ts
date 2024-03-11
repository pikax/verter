import { checkForSetupMethodCall, retrieveNodeString } from "../helpers.js";
import { LocationType, PluginOption, TypeLocation } from "../types.js";

export default {
  name: "Model",

  walk: (node, context) => {
    if (!context.isSetup) return;

    const expression = checkForSetupMethodCall("defineModel", node);
    if (!expression) return;

    const source = context.script!.loc.source;
    const [nameOrOptions, options] = expression.arguments;

    const hasNamedArgument = nameOrOptions?.type === "StringLiteral";
    const propTypeArgument =
      options?.type === "ObjectExpression"
        ? options
        : !hasNamedArgument &&
          nameOrOptions?.type === "ObjectExpression" &&
          nameOrOptions;

    const typeParameter = retrieveNodeString(
      expression.typeParameters?.params?.[0],
      source
    );

    const name = (hasNamedArgument && nameOrOptions.value) || "modelValue";
    const type = propTypeArgument
      ? retrieveNodeString(propTypeArgument, source)!
      : typeParameter || "any";

    // if it's options based declaration
    // we need to create a variable to resolve the type of defineModel
    // then return it and use it in the props and emits
    if (propTypeArgument) {
      const typeName = getModelVarName(name);
      return [
        {
          type: LocationType.Import,
          generated: true,
          node: propTypeArgument,
          // TODO change the import location
          from: "<helpers>",
          items: [
            {
              name: "ExtractModelType",
              type: true,
            },
          ],
        },
        // get the variable from defineModel
        {
          type: LocationType.Declaration,
          node: propTypeArgument,
          generated: true,

          declaration: {
            name: typeName,
            content: retrieveNodeString(expression, source),
          },
        },
        // get the type from the variable
        {
          type: LocationType.Declaration,
          node: propTypeArgument,
          generated: true,
          declaration: {
            name: `TYPE_${typeName}`,
            type: "type",
            content: `ExtractModelType<typeof ${typeName}>;`,
          },
        },
        {
          type: LocationType.Emits,
          node: propTypeArgument,
          generated: true,
          properties: [
            {
              name: `'update:${name}'`,
              content: `TYPE_${typeName}`,
            },
          ],
        },
        {
          type: LocationType.Props,
          node: propTypeArgument,
          generated: true,
          properties: [
            {
              name,
              content: `TYPE_${typeName}`,
            },
          ],
        },
      ];
    }
    return [
      {
        type: LocationType.Emits,
        node: expression,
        properties: [
          {
            name: `'update:${name}'`,
            content: type,
          },
        ],
      },
      {
        type: LocationType.Props,
        node: expression,
        properties: [
          {
            name,
            content: type,
          },
        ],
      },
    ];
  },
} satisfies PluginOption;

export function getModelVarName(name: string) {
  return `__model_${name}`;
}
