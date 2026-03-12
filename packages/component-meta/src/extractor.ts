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
  ApiStyle,
  SlotBinding,
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
  styles: unknown[];
  template: RawTemplate | null;
  optionsApi?: RawOptionsApi | null;
}

interface RawImport {
  source: string;
  isTypeOnly: boolean;
  bindings: { name: string; isTypeOnly: boolean }[];
}

interface RawBinding {
  name: string;
  kind: string;
  typeAnnotation?: string | null;
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
  spanStart: number;
  spanEnd: number;
}

interface RawPropField {
  name: string;
  typeAnnotation?: string | null;
  spanStart: number;
  spanEnd: number;
}

interface RawEmitField {
  name: string;
  spanStart: number;
  spanEnd: number;
}

interface RawMacroTypeDep {
  typeName: string;
  importSource: string;
  macroKind: string;
}

interface RawTemplate {
  propDefinitions?: RawPropDefinition[];
  emitDefinitions?: RawEmitDefinition[];
  definedSlots?: RawDefinedSlot[];
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
}

interface RawOptionsField {
  name: string;
}

// ── Script flags (matches verter_analysis AnalysisFlags) ─────────

const HAS_DEFINE_PROPS = 1 << 0;
const HAS_DEFINE_EMITS = 1 << 1;
const HAS_OPTIONS_API = 1 << 16;

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
 * Extract from a pre-fetched analysis snapshot (useful when you already have
 * the snapshot from an unplugin pipeline or test).
 */
export function snapshotToMeta(snapshot: unknown, filePath: string): ComponentMeta {
  const raw = snapshot as RawSnapshot;
  const apiStyle = detectApiStyle(raw);
  const componentName = deriveComponentName(filePath);

  let props: PropMeta[];
  let events: EventMeta[];
  let slots: SlotMeta[];
  let models: ModelMeta[];
  let exposed: ExposedMeta[];

  if (apiStyle === "options") {
    props = extractOptionsProps(raw.optionsApi);
    events = extractOptionsEmits(raw.optionsApi, raw.template);
    slots = extractSlots(raw.template);
    models = [];
    exposed = extractOptionsExpose(raw.optionsApi);
  } else {
    props = extractCompositionProps(raw.macros, raw.template);
    events = extractCompositionEmits(raw.macros, raw.template);
    slots = extractSlots(raw.template);
    models = extractModels(raw.macros);
    exposed = extractExpose(raw.macros, raw.bindings);
  }

  return {
    filePath,
    componentName,
    apiStyle,
    props,
    events,
    slots,
    models,
    exposed,
  };
}

// ── API style detection ──────────────────────────────────────────

function detectApiStyle(raw: RawSnapshot): ApiStyle {
  const hasComposition =
    (raw.scriptFlags & HAS_DEFINE_PROPS) !== 0 ||
    (raw.scriptFlags & HAS_DEFINE_EMITS) !== 0 ||
    raw.macros.some(
      (m) =>
        m.kind === "DefineProps" ||
        m.kind === "DefineEmits" ||
        m.kind === "DefineModel" ||
        m.kind === "DefineExpose" ||
        m.kind === "DefineSlots",
    );

  const hasOptions = (raw.scriptFlags & HAS_OPTIONS_API) !== 0 || raw.optionsApi != null;

  if (hasComposition && hasOptions) return "mixed";
  if (hasOptions) return "options";
  return "composition";
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

  // Also check withDefaults macro
  const withDefaults = macros.find((m) => m.kind === "WithDefaults");

  return propFields.map((field): PropMeta => {
    const templateDef = defMap.get(field.name);
    const rawType = field.typeAnnotation ?? templateDef?.typeAnnotation ?? undefined;
    const type = rawType ? parseType(rawType) : unknown("unknown");
    const isBoolean = templateDef?.isBoolean ?? false;

    return {
      name: field.name,
      type: isBoolean && type.kind === "unknown" ? primitive("boolean") : type,
      required: templateDef?.isRequired ?? !templateDef?.hasDefault ?? true,
      hasDefault: templateDef?.hasDefault ?? withDefaults != null,
      ...(rawType && { rawType }),
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
    return {
      name: field.name,
      payload: unknown("unknown"),
      hasValidator: templateDef?.hasValidator ?? false,
      isDeclared: templateDef?.isDeclared ?? true,
    };
  });
}

function extractSlots(template: RawTemplate | null): SlotMeta[] {
  if (!template?.definedSlots) return [];

  return template.definedSlots.map((slot): SlotMeta => {
    const bindings: SlotBinding[] = (slot.bindingNames ?? []).map(
      (name): SlotBinding => ({
        name,
        type: unknown("unknown"),
      }),
    );

    return {
      name: slot.name,
      isScoped: slot.hasBindings,
      bindings,
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

  // defineExpose does not list individual fields in the macro analysis.
  // The exposed bindings are the ones passed to the macro call.
  // For now, we return an empty list — this will be populated in Phase 2
  // when a dedicated `getComponentMeta()` Rust API can return them.
  return [];
}

// ── Options API extraction ───────────────────────────────────────

function extractOptionsProps(optionsApi: RawOptionsApi | null | undefined): PropMeta[] {
  if (!optionsApi?.props) return [];

  return optionsApi.props.map((prop): PropMeta => {
    let type: TypeDescriptor;
    const runtimeTypes: string[] = [];

    if (prop.typeConstructor) {
      runtimeTypes.push(prop.typeConstructor);
      type = runtimeTypeToDescriptor(prop.typeConstructor);
    } else {
      type = unknown("unknown");
    }

    return {
      name: prop.name,
      type,
      required: prop.isRequired,
      hasDefault: prop.hasDefault,
      ...(runtimeTypes.length > 0 && { runtimeTypes }),
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
    return {
      name: field.name,
      payload: unknown("unknown"),
      hasValidator: templateDef?.hasValidator ?? false,
      isDeclared: templateDef?.isDeclared ?? true,
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
