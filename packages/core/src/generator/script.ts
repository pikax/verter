import {
  SFCScriptBlock,
  SFCParseResult,
  compileScript,
} from "@vue/compiler-sfc";

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

  compiled.scriptSetupAst;
  const content = compiled.content;

  const generic = script.attrs.generic;

  const models = Array.from(resolveModels(compiled));

  const props = resolveProps(compiled, models);
  const slots = resolveSlots(compiled);
  const emits = resolveEmits(compiled, models);

  return wrapGeneric(content, generic, props, emits, slots);
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
  props: string,
  emits: string,
  slots: string = "{}"
) {
  const component = wrapWithDefineComponent(content);
  // const component = `const ${
  //   VariableName.Options
  // } = defineComponent(${removeExportsAndDefineComponent(content)})`;

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
      }, $emit: ${emits} , $children: ${_slots}  } }`
    : _props;

  // NOTE when generic we need to pass empty values for DeclareComponent Params,
  // this is needed to make sure typescript does not override the scrict type of the component
  const emit = generic ? "{}" : _emits;
  const declareComponent = `DeclareComponent<${genericOrProps},${_data}, ${emit}, ${_slots}, ${_options}>`;

  return `type ${VariableName.InternalComponent} = ${declareComponent}; ${component};`;
}

function resolveProps(scriptSetup: SFCScriptBlock, models: ResolvedModel[]) {
  const modelProps = resolveModelProps(models);

  const props = resolveCompilerFunctions(scriptSetup, "defineProps").next()
    .value;
  if (!props) return ["", modelProps].filter(Boolean).join(" & ");

  if (props.typeParameters) {
    return [props.typeParameters[0], modelProps].filter(Boolean).join(" & ");
  }

  if (props.arguments?.[0]) {
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
  return `DeclareEmits<{${emits}}>`;
}

function resolveSlots(scriptSetup: SFCScriptBlock) {
  const slots = resolveCompilerFunctions(scriptSetup, "defineSlots").next()
    .value;
  if (!slots) return "";

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
    return [`DeclareEmits<${emits.typeParameters[0]}>`, modelEmits]
      .filter(Boolean)
      .join(" & ");
  }

  throw Error("Emits type parameters not found");
  return `DeclareEmits<${emits}>`;
}

function* resolveModels(scriptSetup: SFCScriptBlock) {
  for (const it of resolveCompilerFunctions(scriptSetup, "defineModel")) {
    // the argument is a string, we need to remove the quotes
    const name = it.arguments?.[0]?.slice(1, -1) ?? "modelValue";
    const type = it.typeParameters?.[0] ?? it.arguments?.[1] ?? "any";
    yield {
      name,
      type,
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
  statements: SFCScriptBlock["scriptSetupAst"],
  source: string,
  name: VueAPISetup
): Generator<{
  name: string;
  arguments?: string[];
  typeParameters?: string[];
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
          arguments: statement.expression.arguments?.map((x) =>
            retrieveString(x, source)
          ),
          typeParameters: statement.expression.typeParameters?.params.map((x) =>
            retrieveString(x, source)
          ),
        };
      }
    }

    // @ts-expect-error some error
    if (statement.declarations && statement.declarations.length) {
      // @ts-expect-error some error
      for (let d = 0; d < statement.declarations.length; d++) {
        // @ts-expect-error some error
        const declaration = statement.declarations[d];
        if (
          declaration.init.type === "CallExpression" &&
          declaration.init.callee.name === name
        ) {
          yield {
            name,
            // @ts-expect-error
            arguments: declaration.init.arguments?.map((x) =>
              retrieveString(x, source)
            ),
            typeParameters:
              // @ts-expect-error
              declaration.init.typeParameters?.params.map((x) =>
                retrieveString(x, source)
              ),
          };
        }
      }
    }
  }
}

function retrieveString(node: any, source: string) {
  return source.slice(node.start, node.end);
}
