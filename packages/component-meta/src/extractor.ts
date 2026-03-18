/**
 * Extracts `ComponentMeta` from a Verter analysis snapshot.
 *
 * Pipeline:
 *   host.getAnalysis(id) → raw snapshot → map macros/template/options → resolve types → ComponentMeta
 */

import type { TypeDescriptor } from "./type-ir.js";
import { primitive, union, unknown } from "./type-ir.js";
import type {
  ComponentMeta,
  PropMeta,
  EventMeta,
  SlotMeta,
  ModelMeta,
  ExposedMeta,
  SlotBinding,
  ComponentPropUsage,
  ComponentUsage,
  TemplateRefMeta,
  ImportMeta,
  BindingMeta,
  VueApiCallMeta,
  StyleMeta,
  SelectorMeta,
  ComponentFlags,
} from "./types.js";
import type { VerterHostAdapter } from "./host-adapter.js";
import { parseType, runtimeTypeToDescriptor } from "./resolver.js";

// ── Raw snapshot interfaces (matching Rust serde output) ─────────

/** Mirrors `FileAnalysisSnapshot` from verter_host. */
interface RawSnapshot {
  imports: RawImport[];
  bindings: RawBinding[];
  macros: RawMacro[];
  macroTypeDeps: RawMacroTypeDep[];
  scriptFlags: number;
  styles: RawStyleBlock[];
  template: RawTemplate | null;
  optionsApi?: RawOptionsApi | null;
  vueApiCalls?: RawVueApiCall[];
}

interface RawImport {
  source: string;
  isTypeOnly: boolean;
  bindings: { name: string; isTypeOnly: boolean }[];
}

interface RawBinding {
  name: string;
  kind: string;
  reactivityKind?: string;
  typeAnnotation?: string | null;
  usedInScript?: boolean;
  usedInStyle?: boolean;
}

interface RawMacro {
  kind: string;
  isTypeBased: boolean;
  typeReferences: string[];
  bindingName?: string | null;
  modelName?: string | null;
  hasInheritAttrsFalse: boolean;
  propFields?: RawPropField[];
  emitFields?: RawEmitField[];
  slotFields?: RawSlotField[];
  defaultKeys?: string[];
  defaultValues?: RawDefaultValue[];
  exposeFields?: RawExposeField[];
  resolvedLocalTypes?: RawResolvedLocalType[];
  spanStart: number;
  spanEnd: number;
}

interface RawDefaultValue {
  key: string;
  value: string;
}

interface RawResolvedLocalType {
  name: string;
  expanded: string;
}

interface RawExposeField {
  name: string;
  spanStart: number;
  spanEnd: number;
}

interface RawSlotField {
  name: string;
  isRequired: boolean;
  bindings?: Array<{ name: string; typeAnnotation?: string | null }>;
  returnType?: string | null;
  description?: string | null;
  tags?: RawJsdocTag[];
}

interface RawJsdocTag {
  name: string;
  text?: string | null;
}

interface RawPropField {
  name: string;
  isOptional?: boolean;
  typeAnnotation?: string | null;
  description?: string | null;
  tags?: RawJsdocTag[];
  resolutionSource?: string;
  resolutionError?: string | null;
  spanStart: number;
  spanEnd: number;
}

interface RawEmitField {
  name: string;
  payloadType?: string | null;
  description?: string | null;
  tags?: RawJsdocTag[];
  spanStart: number;
  spanEnd: number;
}

interface RawMacroTypeDep {
  typeName: string;
  importSource: string;
  macroKind: string;
}

interface RawTemplate {
  components?: RawComponentUsage[];
  templateRefs?: RawTemplateRef[];
  bindingOccurrences?: RawBindingOccurrence[];
  propDefinitions?: RawPropDefinition[];
  emitDefinitions?: RawEmitDefinition[];
  definedSlots?: RawDefinedSlot[];
}

interface RawComponentUsage {
  name: string;
  importSource?: string;
  isDynamic: boolean;
  props?: RawTemplateProp[];
  slotsUsed?: string[];
  staticClasses?: string[];
  hasDynamicClass: boolean;
  vModels?: RawTemplateVModel[];
}

interface RawTemplateProp {
  name: string;
  isBound?: boolean;
  constness?: string;
}

interface RawTemplateVModel {
  bindingName: string;
}

interface RawTemplateRef {
  name: string;
  isDynamic: boolean;
  targetTag?: string;
}

interface RawBindingOccurrence {
  name: string;
}

interface RawPropDefinition {
  name: string;
  typeAnnotation?: string | null;
  hasDefault: boolean;
  isRequired: boolean;
  isBoolean: boolean;
  usedInTemplate: boolean;
  usedInScript: boolean;
}

interface RawEmitDefinition {
  eventName: string;
  hasValidator: boolean;
  isDeclared: boolean;
}

interface RawDefinedSlot {
  name: string;
  hasBindings: boolean;
  bindingNames?: string[];
  bindingExpressions?: string[];
  hasFallbackContent?: boolean;
}

interface RawOptionsApi {
  isDefineComponent: boolean;
  props?: RawOptionsProp[];
  emits?: RawEmitField[];
  expose?: RawOptionsField[];
}

interface RawOptionsProp {
  name: string;
  typeConstructor?: string | null;
  isRequired: boolean;
  hasDefault: boolean;
  defaultValue?: string | null;
  typeAnnotation?: string | null;
  description?: string | null;
  tags?: RawJsdocTag[];
}

interface RawOptionsField {
  name: string;
}

interface RawStyleBlock {
  lang?: string;
  scoped?: boolean;
  isModule?: boolean;
  moduleName?: string | null;
  vBinds?: RawVBind[];
  css?: RawCssAnalysis | null;
}

interface RawVBind {
  expression: string;
}

interface RawCssAnalysis {
  selectors?: RawSelector[];
  classes?: RawCssClass[];
  ids?: RawCssId[];
  customProperties?: RawCustomProperty[];
}

interface RawSelector {
  text: string;
  specificity: [number, number, number];
}

interface RawCssClass {
  name: string;
}

interface RawCssId {
  name: string;
}

interface RawCustomProperty {
  name: string;
}

interface RawVueApiCall {
  api: string;
  argValue?: string;
}

// ── Script flags (matches verter_analysis AnalysisFlags) ─────────

const ASYNC_SETUP = 1 << 0;
const HAS_DEFINE_PROPS = 1 << 1;
const HAS_DEFINE_EMITS = 1 << 2;
const HAS_REACTIVE_STATE = 1 << 11;
const HAS_COMPUTED = 1 << 12;
const HAS_WATCHERS = 1 << 13;
const HAS_LIFECYCLE_HOOKS = 1 << 14;
const HAS_PROVIDE = 1 << 15;
const HAS_INJECT = 1 << 16;
const HAS_INHERIT_ATTRS_FALSE = 1 << 18;
const HAS_OPTIONS_API = 1 << 19;
const HAS_STORE_USAGE = 1 << 20;

// ── Extraction ───────────────────────────────────────────────────

/**
 * Extract `ComponentMeta` for a single file.
 *
 * @param adapter  Host adapter (NAPI or WASM wrapped)
 * @param fileId   Canonical ID or alias for the file
 * @param filePath Display file path (for the result)
 */
export function extractComponentMeta(
  adapter: VerterHostAdapter,
  fileId: string,
  filePath?: string,
): ComponentMeta | null {
  const raw = adapter.getAnalysis(fileId) as RawSnapshot | null;
  if (!raw) return null;
  return snapshotToMeta(raw, filePath ?? fileId);
}

/**
 * Build a type registry mapping type names to their expanded text.
 * Used by the schema layer to resolve `ref` types.
 */
export function buildTypeRegistry(snapshot: unknown): Map<string, string> {
  const raw = snapshot as RawSnapshot;
  const registry = new Map<string, string>();
  for (const macro of raw.macros) {
    for (const rlt of macro.resolvedLocalTypes ?? []) {
      registry.set(rlt.name, rlt.expanded);
    }
  }
  return registry;
}

/**
 * Extract from a pre-fetched analysis snapshot (useful when you already have
 * the snapshot from an unplugin pipeline or test).
 */
export function snapshotToMeta(snapshot: unknown, filePath: string): ComponentMeta {
  const raw = snapshot as RawSnapshot;
  const optionsApi = detectOptionsApi(raw);
  const componentName = deriveComponentName(filePath);

  let props: PropMeta[];
  let events: EventMeta[];
  let slots: SlotMeta[];
  let models: ModelMeta[];
  let exposed: ExposedMeta[];

  if (optionsApi && !hasCompositionMacros(raw)) {
    props = extractOptionsProps(raw.optionsApi);
    events = extractOptionsEmits(raw.optionsApi, raw.template);
    slots = extractSlots(raw.template, raw.macros, raw.bindings);
    models = [];
    exposed = extractOptionsExpose(raw.optionsApi);
  } else {
    props = extractCompositionProps(raw.macros, raw.template);
    events = extractCompositionEmits(raw.macros, raw.template);
    slots = extractSlots(raw.template, raw.macros, raw.bindings);
    models = extractModels(raw.macros);
    exposed = extractExpose(raw.macros, raw.bindings);

    // Synthesize implicit props and events from defineModel macros
    const modelMacros = raw.macros.filter((m) => m.kind === "DefineModel");
    for (const m of modelMacros) {
      const modelName = m.modelName ?? "modelValue";
      // Add model prop if not already in props list
      if (!props.some((p) => p.name === modelName)) {
        const propField = m.propFields?.[0];
        const rawType = propField?.typeAnnotation ?? undefined;
        const hasDefault = (m.defaultKeys ?? []).includes(modelName);
        const isModelOptional = hasDefault;
        const finalRawType = isModelOptional && rawType ? `${rawType} | undefined` : rawType;
        props.push({
          name: modelName,
          type:
            isModelOptional && rawType
              ? union([parseType(rawType), primitive("undefined")])
              : rawType
                ? parseType(rawType)
                : unknown("unknown"),
          required: !hasDefault,
          hasDefault,
          ...(finalRawType && { rawType: finalRawType }),
        });
      }
      // Add update:modelName event if not already in events list
      const updateEventName = `update:${modelName}`;
      if (!events.some((e) => e.name === updateEventName)) {
        const propField = m.propFields?.[0];
        const rawType = propField?.typeAnnotation ?? undefined;
        const tupleSignature = rawType ? `[value: ${rawType}]` : undefined;
        events.push({
          name: updateEventName,
          payload: tupleSignature ? parseType(tupleSignature) : unknown("unknown"),
          hasValidator: false,
          isDeclared: true,
          ...(tupleSignature && { rawSignature: tupleSignature }),
        });
      }
    }
  }

  return {
    filePath,
    componentName,
    optionsApi,
    props,
    events,
    slots,
    models,
    exposed,
    components: extractComponents(raw.template),
    templateRefs: extractTemplateRefs(raw.template),
    imports: extractImports(raw.imports),
    bindings: extractBindings(raw.bindings, raw.template),
    vueApiCalls: extractVueApiCalls(raw.vueApiCalls),
    styles: extractStyles(raw.styles),
    flags: extractFlags(raw.scriptFlags),
  };
}

// ── API style detection ──────────────────────────────────────────

function detectOptionsApi(raw: RawSnapshot): boolean {
  return (raw.scriptFlags & HAS_OPTIONS_API) !== 0 || raw.optionsApi != null;
}

function hasCompositionMacros(raw: RawSnapshot): boolean {
  return (
    (raw.scriptFlags & HAS_DEFINE_PROPS) !== 0 ||
    (raw.scriptFlags & HAS_DEFINE_EMITS) !== 0 ||
    raw.macros.some(
      (m) =>
        m.kind === "DefineProps" ||
        m.kind === "DefineEmits" ||
        m.kind === "DefineModel" ||
        m.kind === "DefineExpose" ||
        m.kind === "DefineSlots",
    )
  );
}

// ── Component name derivation ────────────────────────────────────

function deriveComponentName(filePath: string): string {
  // "/path/to/MyComponent.vue" → "MyComponent"
  const parts = filePath.replace(/\\/g, "/").split("/");
  const filename = parts[parts.length - 1] ?? "Component";
  return filename.replace(/\.vue$/i, "");
}

// ── Composition API extraction ───────────────────────────────────

function extractCompositionProps(macros: RawMacro[], template: RawTemplate | null): PropMeta[] {
  const defineProps = macros.find((m) => m.kind === "DefineProps");
  if (!defineProps) return [];

  const propFields = defineProps.propFields ?? [];
  const propDefs = template?.propDefinitions ?? [];

  // Build a lookup from template propDefinitions for extra info
  const defMap = new Map<string, RawPropDefinition>();
  for (const def of propDefs) {
    defMap.set(def.name, def);
  }

  // Build set of default keys and values from withDefaults macro or runtime defineProps defaults
  const withDefaults = macros.find((m) => m.kind === "WithDefaults");
  const defaultKeys = new Set<string>([
    ...(withDefaults?.defaultKeys ?? []),
    ...(defineProps.defaultKeys ?? []),
  ]);
  const defaultValueMap = new Map<string, string>();
  for (const dv of withDefaults?.defaultValues ?? []) {
    defaultValueMap.set(dv.key, dv.value);
  }
  for (const dv of defineProps.defaultValues ?? []) {
    if (!defaultValueMap.has(dv.key)) {
      defaultValueMap.set(dv.key, dv.value);
    }
  }

  return propFields.map((field): PropMeta => {
    const templateDef = defMap.get(field.name);
    const baseRawType = field.typeAnnotation ?? templateDef?.typeAnnotation ?? undefined;
    const isOptional = field.isOptional ?? false;
    const rawType = isOptional && baseRawType ? `${baseRawType} | undefined` : baseRawType;
    const baseType = baseRawType ? parseType(baseRawType) : unknown("unknown");
    const type =
      isOptional && baseType.kind !== "unknown"
        ? union([baseType, primitive("undefined")])
        : baseType;
    const isBoolean = templateDef?.isBoolean ?? false;
    const description = field.description ?? undefined;
    const tags = field.tags?.map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    }));

    const hasDefault = templateDef?.hasDefault ?? defaultKeys.has(field.name);
    const required =
      templateDef != null
        ? (templateDef.isRequired ?? !templateDef.hasDefault)
        : !isOptional && !hasDefault;

    const defaultValue = defaultValueMap.get(field.name);

    return {
      name: field.name,
      type: isBoolean && type.kind === "unknown" ? primitive("boolean") : type,
      required,
      hasDefault,
      ...(rawType && { rawType }),
      ...(description && { description }),
      ...(tags && tags.length > 0 && { tags }),
      ...(defaultValue != null && { default: defaultValue }),
    };
  });
}

function extractCompositionEmits(macros: RawMacro[], template: RawTemplate | null): EventMeta[] {
  const defineEmits = macros.find((m) => m.kind === "DefineEmits");
  if (!defineEmits) return [];

  const emitFields = defineEmits.emitFields ?? [];
  const emitDefs = template?.emitDefinitions ?? [];

  const defMap = new Map<string, RawEmitDefinition>();
  for (const def of emitDefs) {
    defMap.set(def.eventName, def);
  }

  return emitFields.map((field): EventMeta => {
    const templateDef = defMap.get(field.name);
    const description = field.description ?? undefined;
    const rawPayload = field.payloadType ?? undefined;
    const payload = rawPayload ? parseType(rawPayload) : unknown("unknown");
    const tags = field.tags?.map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    }));
    return {
      name: field.name,
      payload,
      hasValidator: templateDef?.hasValidator ?? false,
      isDeclared: templateDef?.isDeclared ?? true,
      ...(rawPayload && { rawSignature: rawPayload }),
      ...(description && { description }),
      ...(tags && tags.length > 0 && { tags }),
    };
  });
}

function extractSlots(
  template: RawTemplate | null,
  macros: RawMacro[],
  scriptBindings?: RawBinding[],
): SlotMeta[] {
  // If template has no <slot> tags but defineSlots exists, use it as primary source
  if (!template?.definedSlots || template.definedSlots.length === 0) {
    const defineSlotsM = macros.find(
      (m) => m.kind === "DefineSlots" && m.slotFields && m.slotFields.length > 0,
    );
    if (defineSlotsM?.slotFields) {
      return defineSlotsM.slotFields.map((sf): SlotMeta => {
        const bindings: SlotBinding[] = (sf.bindings ?? []).map((b): SlotBinding => {
          const rawType = b.typeAnnotation ?? undefined;
          return {
            name: b.name,
            type: rawType ? parseType(rawType) : unknown("unknown"),
            ...(rawType && { rawType }),
          };
        });
        const desc = sf.description ?? undefined;
        const sfTags = sf.tags?.map((t) => ({
          name: t.name,
          ...(t.text != null && { text: t.text }),
        }));
        const returnType = sf.returnType ?? undefined;
        return {
          name: sf.name,
          isScoped: (sf.bindings ?? []).length > 0,
          bindings,
          ...(sf.isRequired != null && { isRequired: sf.isRequired }),
          ...(returnType && { returnType }),
          ...(desc && { description: desc }),
          ...(sfTags && sfTags.length > 0 && { tags: sfTags }),
        };
      });
    }
    return [];
  }

  // Build maps from defineSlots macro: isRequired + binding types + jsdoc per slot
  const requiredMap = new Map<string, boolean>();
  const slotJsdocMap = new Map<
    string,
    { description?: string; tags?: { name: string; text?: string }[] }
  >();
  const slotBindingMap = new Map<string, Map<string, string>>();
  for (const m of macros) {
    if (m.kind === "DefineSlots" && m.slotFields) {
      for (const sf of m.slotFields) {
        requiredMap.set(sf.name, sf.isRequired);
        const desc = sf.description ?? undefined;
        const sfTags = sf.tags?.map((t) => ({
          name: t.name,
          ...(t.text != null && { text: t.text }),
        }));
        if (desc || (sfTags && sfTags.length > 0)) {
          slotJsdocMap.set(sf.name, {
            ...(desc && { description: desc }),
            ...(sfTags && sfTags.length > 0 && { tags: sfTags }),
          });
        }
        if (sf.bindings && sf.bindings.length > 0) {
          const bindingTypeMap = new Map<string, string>();
          for (const b of sf.bindings) {
            if (b.typeAnnotation) {
              bindingTypeMap.set(b.name, b.typeAnnotation);
            }
          }
          if (bindingTypeMap.size > 0) {
            slotBindingMap.set(sf.name, bindingTypeMap);
          }
        }
      }
    }
  }

  // Build a lookup from script bindings for fallback type resolution
  const scriptBindingTypes = new Map<string, string>();
  if (scriptBindings) {
    for (const b of scriptBindings) {
      if (b.typeAnnotation) {
        scriptBindingTypes.set(b.name, b.typeAnnotation);
      }
    }
  }

  return template.definedSlots.map((slot): SlotMeta => {
    const slotTypeMap = slotBindingMap.get(slot.name);

    const bindings: SlotBinding[] = (slot.bindingNames ?? []).map((name, i): SlotBinding => {
      const expression = slot.bindingExpressions?.[i] ?? undefined;

      // 1. Primary: defineSlots slotFields bindings
      let rawType: string | undefined = slotTypeMap?.get(name);

      // 2. Fallback: cross-reference expression with script bindings
      if (!rawType && expression) {
        rawType = scriptBindingTypes.get(expression) ?? undefined;
      }

      const type = rawType ? parseType(rawType) : unknown("unknown");

      return {
        name,
        type,
        ...(expression != null && { expression }),
        ...(rawType && { rawType }),
      };
    });

    const jsdoc = slotJsdocMap.get(slot.name);
    return {
      name: slot.name,
      isScoped: slot.hasBindings,
      bindings,
      ...(requiredMap.has(slot.name) && { isRequired: requiredMap.get(slot.name) }),
      ...(slot.hasFallbackContent && { hasFallbackContent: true }),
      ...(jsdoc?.description && { description: jsdoc.description }),
      ...(jsdoc?.tags && jsdoc.tags.length > 0 && { tags: jsdoc.tags }),
    };
  });
}

function extractModels(macros: RawMacro[]): ModelMeta[] {
  return macros
    .filter((m) => m.kind === "DefineModel")
    .map((m): ModelMeta => {
      const name = m.modelName ?? "modelValue";
      // Model type from propFields if present
      const propField = m.propFields?.[0];
      const rawType = propField?.typeAnnotation;
      return {
        name,
        type: rawType ? parseType(rawType) : unknown("unknown"),
      };
    });
}

function extractExpose(macros: RawMacro[], bindings: RawBinding[]): ExposedMeta[] {
  const defineExpose = macros.find((m) => m.kind === "DefineExpose");
  if (!defineExpose) return [];

  const exposeFields = defineExpose.exposeFields ?? [];
  if (exposeFields.length > 0) {
    // Build type lookup from script bindings
    const bindingTypes = new Map<string, string>();
    for (const b of bindings) {
      if (b.typeAnnotation) {
        bindingTypes.set(b.name, b.typeAnnotation);
      }
    }
    return exposeFields.map((field): ExposedMeta => {
      const rawType = bindingTypes.get(field.name);
      return {
        name: field.name,
        type: rawType ? parseType(rawType) : unknown("unknown"),
      };
    });
  }

  return [];
}

// ── Options API extraction ───────────────────────────────────────

function extractOptionsProps(optionsApi: RawOptionsApi | null | undefined): PropMeta[] {
  if (!optionsApi?.props) return [];

  return optionsApi.props.map((prop): PropMeta => {
    let type: TypeDescriptor;
    const runtimeTypes: string[] = [];

    if (prop.typeAnnotation) {
      // Use PropType<T> annotation if available (e.g., `Object as PropType<HTMLCanvasElement>`)
      type = parseType(prop.typeAnnotation);
    } else if (prop.typeConstructor) {
      runtimeTypes.push(prop.typeConstructor);
      type = runtimeTypeToDescriptor(prop.typeConstructor);
    } else {
      type = unknown("unknown");
    }

    const description = prop.description ?? undefined;
    const tags = prop.tags?.map((t) => ({
      name: t.name,
      ...(t.text != null && { text: t.text }),
    }));

    return {
      name: prop.name,
      type,
      required: prop.isRequired,
      hasDefault: prop.hasDefault,
      ...(runtimeTypes.length > 0 && { runtimeTypes }),
      ...(prop.typeAnnotation && { rawType: prop.typeAnnotation }),
      ...(prop.defaultValue != null && { default: prop.defaultValue }),
      ...(description && { description }),
      ...(tags && tags.length > 0 && { tags }),
    };
  });
}

function extractOptionsEmits(
  optionsApi: RawOptionsApi | null | undefined,
  template: RawTemplate | null,
): EventMeta[] {
  if (!optionsApi?.emits) return [];

  const emitDefs = template?.emitDefinitions ?? [];
  const defMap = new Map<string, RawEmitDefinition>();
  for (const def of emitDefs) {
    defMap.set(def.eventName, def);
  }

  return optionsApi.emits.map((field): EventMeta => {
    const templateDef = defMap.get(field.name);
    const rawPayload = field.payloadType ?? undefined;
    const payload = rawPayload ? parseType(rawPayload) : unknown("unknown");
    return {
      name: field.name,
      payload,
      hasValidator: templateDef?.hasValidator ?? false,
      isDeclared: templateDef?.isDeclared ?? true,
      ...(rawPayload && { rawSignature: rawPayload }),
    };
  });
}

function extractOptionsExpose(optionsApi: RawOptionsApi | null | undefined): ExposedMeta[] {
  if (!optionsApi?.expose) return [];

  return optionsApi.expose.map(
    (field): ExposedMeta => ({
      name: field.name,
      type: unknown("unknown"),
    }),
  );
}

// ── New extraction functions ─────────────────────────────────────

function extractComponents(template: RawTemplate | null): ComponentUsage[] {
  if (!template?.components) return [];

  return template.components.map(
    (comp): ComponentUsage => ({
      name: comp.name,
      ...(comp.importSource && { importSource: comp.importSource }),
      isDynamic: comp.isDynamic,
      props: (comp.props ?? []).map(
        (p): ComponentPropUsage => ({
          name: p.name,
          isBound: p.isBound ?? false,
          constness: mapConstness(p.constness),
        }),
      ),
      slotsUsed: comp.slotsUsed ?? [],
      staticClasses: comp.staticClasses ?? [],
      hasDynamicClass: comp.hasDynamicClass,
      vModels: (comp.vModels ?? []).map((m) => m.bindingName),
    }),
  );
}

function mapConstness(constness: string | undefined): "const" | "dynamic" | "unknown" {
  switch (constness) {
    case "Const":
    case "const":
      return "const";
    case "Dynamic":
    case "dynamic":
      return "dynamic";
    default:
      return "unknown";
  }
}

function extractTemplateRefs(template: RawTemplate | null): TemplateRefMeta[] {
  if (!template?.templateRefs) return [];

  return template.templateRefs.map(
    (ref): TemplateRefMeta => ({
      name: ref.name,
      isDynamic: ref.isDynamic,
      targetTag: ref.targetTag ?? "",
    }),
  );
}

function extractImports(imports: RawImport[]): ImportMeta[] {
  return imports.map(
    (imp): ImportMeta => ({
      source: imp.source,
      isTypeOnly: imp.isTypeOnly,
      bindings: imp.bindings,
    }),
  );
}

function extractBindings(bindings: RawBinding[], template: RawTemplate | null): BindingMeta[] {
  // Build a set of binding names used in template from bindingOccurrences
  const templateBindings = new Set<string>();
  if (template?.bindingOccurrences) {
    for (const occ of template.bindingOccurrences) {
      templateBindings.add(occ.name);
    }
  }

  return bindings.map(
    (b): BindingMeta => ({
      name: b.name,
      kind: mapBindingKind(b.kind),
      reactivityKind: mapReactivityKind(b.reactivityKind),
      ...(b.typeAnnotation != null && { typeAnnotation: b.typeAnnotation }),
      usedInTemplate: templateBindings.has(b.name),
      usedInStyle: b.usedInStyle ?? false,
    }),
  );
}

function mapBindingKind(
  kind: string,
): "const" | "let" | "var" | "function" | "asyncFunction" | "class" {
  switch (kind) {
    case "Const":
    case "const":
      return "const";
    case "Let":
    case "let":
      return "let";
    case "Var":
    case "var":
      return "var";
    case "Function":
    case "function":
      return "function";
    case "AsyncFunction":
    case "asyncFunction":
      return "asyncFunction";
    case "Class":
    case "class":
      return "class";
    default:
      return "const";
  }
}

function mapReactivityKind(
  kind: string | undefined,
): "none" | "ref" | "reactive" | "computed" | "maybeRef" | "mutable" {
  switch (kind) {
    case "Ref":
    case "ref":
      return "ref";
    case "Reactive":
    case "reactive":
      return "reactive";
    case "Computed":
    case "computed":
      return "computed";
    case "MaybeRef":
    case "maybeRef":
      return "maybeRef";
    case "Mutable":
    case "mutable":
      return "mutable";
    default:
      return "none";
  }
}

function extractVueApiCalls(calls: RawVueApiCall[] | undefined): VueApiCallMeta[] {
  if (!calls) return [];

  return calls.map(
    (call): VueApiCallMeta => ({
      api: call.api,
      ...(call.argValue != null && { argValue: call.argValue }),
    }),
  );
}

function extractStyles(styles: RawStyleBlock[]): StyleMeta[] {
  return styles.map((style): StyleMeta => {
    const css = style.css;
    return {
      lang: style.lang ?? "Css",
      scoped: style.scoped ?? false,
      isModule: style.isModule ?? false,
      ...(style.moduleName != null && { moduleName: style.moduleName }),
      classes: (css?.classes ?? []).map((c) => c.name),
      ids: (css?.ids ?? []).map((id) => id.name),
      customProperties: (css?.customProperties ?? []).map((cp) => cp.name),
      vBinds: (style.vBinds ?? []).map((vb) => vb.expression),
      selectors: (css?.selectors ?? []).map(
        (sel): SelectorMeta => ({
          text: sel.text,
          specificity: sel.specificity,
        }),
      ),
    };
  });
}

function extractFlags(scriptFlags: number): ComponentFlags {
  return {
    asyncSetup: (scriptFlags & ASYNC_SETUP) !== 0,
    hasReactiveState: (scriptFlags & HAS_REACTIVE_STATE) !== 0,
    hasComputed: (scriptFlags & HAS_COMPUTED) !== 0,
    hasWatchers: (scriptFlags & HAS_WATCHERS) !== 0,
    hasLifecycleHooks: (scriptFlags & HAS_LIFECYCLE_HOOKS) !== 0,
    hasProvide: (scriptFlags & HAS_PROVIDE) !== 0,
    hasInject: (scriptFlags & HAS_INJECT) !== 0,
    hasInheritAttrsFalse: (scriptFlags & HAS_INHERIT_ATTRS_FALSE) !== 0,
    hasStoreUsage: (scriptFlags & HAS_STORE_USAGE) !== 0,
  };
}
