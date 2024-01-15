import {
  SFCScriptBlock,
  SFCParseResult,
  compileScript,
} from "@vue/compiler-sfc";
import type * as _babel_types from "@babel/types";

export const DEFAULT_FILENAME = "anonymous.vue";

const enum VariableName {
  Options = "ComponentOptions",
  InternalComponent = "__COMP__",
}

type VueAPISetup =
  | "defineProps"
  | "defineEmits"
  | "defineSlots"
  | "defineOptions"
  | "defineModel";

type ResolvedModel = {
  name: string;
  type: string;

  // declares the model as a variable to resolve the type
  declare: boolean;
  declaration?: string;
};

export function generateScript(sfc: SFCParseResult) {
  const isSetup = !!sfc.descriptor.scriptSetup;
  const script = isSetup ? sfc.descriptor.scriptSetup : sfc.descriptor.script;
  // const source = script?.content;
  // const match = filename !== DEFAULT_FILENAME ? filename.match(/([^/\\]+)\.\w+$/) : null;

  if (!script) {
    const component = wrapWithDefineComponent("{ }");
    return `type ${VariableName.InternalComponent} = typeof ${VariableName.Options};\n ${component};`;
  }

  const compiled = compileScript(sfc.descriptor, {
    id: "random-id",
  });

  const content = compiled.content;

  const generic = script.attrs.generic;

  const models = Array.from(resolveModels(compiled));

  const props = resolveProps(compiled, models);
  const slots = resolveSlots(compiled);
  const emits = resolveEmits(compiled, models);

  return wrapGeneric(content, generic, models, props, emits, slots);
}

const possibleExports = [
  "export default /*#__PURE__*/_",
  "export default /*#__PURE__*/",
  "export default ",
];

function removeExportsAndDefineComponent(content: string) {
  for (let i = 0; i < possibleExports.length; i++) {
    const element = possibleExports[i];
    const indexOf = content.indexOf(element);
    if (~indexOf) {
      content = content.slice(indexOf + element.length);
      break;
    }
  }

  if (content.startsWith("defineComponent")) {
    return content.slice("defineComponent".length);
  }
  return content;
}

function wrapWithDefineComponent(content: string) {
  return `const ${
    VariableName.Options
} = defineComponent(${removeExportsAndDefineComponent(content)})`;
}

function wrapGeneric(
  content: string,
  generic: string | boolean,
  models: ResolvedModel[],
  props: string,
  emits: string,
  slots: string = "{}"
) {
  const declarations = models
    .filter((x) => x.declare)
    .map((x) => `const ${getModelVarName(x.name)} = ${x.declaration};`)
    .join("\n");

  const component = wrapWithDefineComponent(content);
  const _props =
    [props, emits && `EmitsToProps<${emits}>`].filter(Boolean).join(" & ") ||
    "{}";

  const _data = `ComponentData<typeof ${VariableName.Options}>`;
  const _emits = emits || "{}";
  const _slots = (slots && `SlotsType<${slots}>`) || "{}";
  const _options = `typeof ${VariableName.Options}`;

  const genericOrProps = generic
    ? `{ new<${generic}>(): { $props: ${
        _props || "{}"
      }, $emit: ${_emits} , $children: ${_slots}  } }`
    : _props;

  // NOTE when generic we need to pass empty values for DeclareComponent Params,
  // this is needed to make sure typescript does not override the scrict type of the component
  const emit = generic ? "{}" : _emits;
  const declareComponent = `DeclareComponent<${genericOrProps},${_data}, ${emit}, ${_slots}, ${_options}>`;

  return `${declarations ? declarations + ";\n\n" : ""}type ${
    VariableName.InternalComponent
  } = ${declareComponent}; ${component};`;
}

function resolveProps(scriptSetup: SFCScriptBlock, models: ResolvedModel[]) {
  const modelProps = resolveModelProps(models);

  const props = resolveCompilerFunctions(scriptSetup, "defineProps").next()
    .value;
  if (!props) return ["", modelProps].filter(Boolean).join(" & ");

  if (props.typeParameters) {
    return [props.typeParameters[0].value, modelProps]
      .filter(Boolean)
      .join(" & ");
  }

  if (props.args?.[0]) {
    return [`typeof ${VariableName.Options}['props']`, modelProps]
      .filter(Boolean)
      .join(" & ");
  }

  throw Error("props type parameters not found");

  // return resolveCompilerFunctions(scriptSetup, 'defineProps').next().value ?? '';
}

function resolveModelProps(models: ResolvedModel[]) {
  const props = models.map((x) => `${x.name}: ${x.type || "any"}`).join("\n");
  if (!props) return "";
  return `{${props}}`;
}
function resolveModelEmits(models: ResolvedModel[]) {
  // using short syntax
  const emits = models
    .map((x) => `['update:${x.name}']: [${x.type || "any"}]`)
    .join("\n");
  if (!emits) return "";
  // return `DeclareEmits<{${emits}}>`;
  return `{${emits}}`;
}

function resolveSlots(scriptSetup: SFCScriptBlock) {
  const slots = resolveCompilerFunctions(scriptSetup, "defineSlots").next()
    .value;
  if (!slots) return undefined;

  if (slots.typeParameters) {
    return slots.typeParameters[0];
  }

  throw Error("Slots type parameters not found");

  // return resolveCompilerFunctions(scriptSetup, 'defineSlots').next().value ?? '';
}
function resolveEmits(scriptSetup: SFCScriptBlock, models: ResolvedModel[]) {
  const modelEmits = resolveModelEmits(models);
  const emits =
    resolveCompilerFunctions(scriptSetup, "defineEmits").next().value ?? "";
  if (!emits) return ["", modelEmits].filter(Boolean).join(" & ");
  if (emits.typeParameters) {
    return [`DeclareEmits<${emits.typeParameters[0].value}>`, modelEmits]
      .filter(Boolean)
      .join(" & ");
  }

  throw Error("Emits type parameters not found");
  return `DeclareEmits<${emits}>`;
}

export function* resolveModels(scriptSetup: SFCScriptBlock) {
  console.log("ddd", scriptSetup);
  for (const it of resolveCompilerFunctions(scriptSetup, "defineModel")) {
    // the argument is a string, we need to remove the quotes

    const hasNamedArgument = it.args?.[0]?.type === "StringLiteral";
    const hasTypeParameter = !!it.typeParameters?.[0];
    const declare = !hasTypeParameter;

    const name = hasNamedArgument ? it.args![0].value : "modelValue";
    const type = hasTypeParameter
      ? it.typeParameters?.[0]?.value!
      : `ExtractModelType<typeof ${getModelVarName(name)}>`;

    console.log("resolved", name, type);
    yield {
      name,
      type,
      declare,
      declaration: declare
        ? scriptSetup.loc.source.slice(it.node.start, it.node.end)
        : undefined,
    } satisfies ResolvedModel;
  }
}

function* resolveCompilerFunctions(
  scriptSetup: SFCScriptBlock,
  name: VueAPISetup
) {
  if (!scriptSetup.scriptSetupAst) return "";

  yield* retrieveFunctionCall(
    scriptSetup.scriptSetupAst,
    scriptSetup.loc.source,
    name
  );
}

function* retrieveFunctionCall(
  statements: _babel_types.Statement[],
  source: string,
  name: VueAPISetup
): Generator<{
  name: string;
  node: _babel_types.CallExpression;
  args?: RetrieveValue[];
  typeParameters?: RetrieveValue[];
}> {
  if (!statements) return undefined;

  for (let i = 0; i < statements.length; i++) {
    const statement = statements[i];

    // without assigning
    if (statement.type === "ExpressionStatement") {
      if (
        statement.expression.type === "CallExpression" &&
        // @ts-expect-error some error
        statement.expression.callee.name === name
      ) {
        yield {
          name,
          node: statement.expression,
          args: statement.expression.arguments?.map((x) =>
            retriveValue(x, source)
          ),
          typeParameters: statement.expression.typeParameters?.params.map((x) =>
            retriveValue(x, source)
          ),
        };
      }
    }

    if (
      statement.type === "VariableDeclaration" &&
      statement.declarations &&
      statement.declarations.length
    ) {
      if (statement.declarations && statement.declarations.length) {
        for (let d = 0; d < statement.declarations.length; d++) {
          const declaration = statement.declarations[d];
          if (
            declaration?.init?.type === "CallExpression" &&
            // @ts-expect-error .name not inferred here
            declaration.init.callee.name === name
          ) {
            yield {
              name,
              node: declaration.init,
              args: declaration.init.arguments?.map((x) =>
                retriveValue(x, source)
              ),
              typeParameters: declaration.init.typeParameters?.params.map((x) =>
                retriveValue(x, source)
              ),
            };
          }
        }
      }
    }
  }
}

function retrieveString(node: any, source: string) {
  return source.slice(node.start, node.end);
}

type ValueNode =
  | _babel_types.Expression
  | _babel_types.SpreadElement
  | _babel_types.JSXNamespacedName
  | _babel_types.ArgumentPlaceholder
  | _babel_types.TSType;
type RetrieveValue = {
  value: string;
  type: _babel_types.StringLiteral["type"] | string;
  node: ValueNode;
};

function retriveValue(
  node:
    | _babel_types.Expression
    | _babel_types.SpreadElement
    | _babel_types.JSXNamespacedName
    | _babel_types.ArgumentPlaceholder
    | _babel_types.TSType,
  source: string
): RetrieveValue {
  const value =
    (node.type === "StringLiteral" && node.value) ||
    source.slice(node.start ?? 0, node.end ?? 0);
  const type = node.type;

  return {
    value,
    type,

    node,
  };
}

function getModelVarName(name: string) {
  return `__model_${name}`;
}
