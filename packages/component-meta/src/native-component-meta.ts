import { basename } from "node:path";

import { typeExprToDescriptor } from "./type-expr-bridge.js";
import type { ComponentMeta } from "./types.js";
import type { TypeDescriptor } from "./type-ir.js";
import type { NativeTypeExpr } from "./type-expr-bridge.js";

interface NativeJsdocTag {
  name: string;
  text?: string;
}

interface NativePropMeta {
  name: string;
  type: NativeTypeExpr;
  rawType?: string;
  required: boolean;
  hasDefault: boolean;
  defaultValue?: string;
  description?: string;
  tags?: NativeJsdocTag[];
}

interface NativeEventMeta {
  name: string;
  payload: NativeTypeExpr;
  rawSignature?: string;
  description?: string;
  tags?: NativeJsdocTag[];
}

interface NativeSlotBindingMeta {
  name: string;
  type: NativeTypeExpr;
  rawType?: string;
}

interface NativeSlotMeta {
  name: string;
  isScoped: boolean;
  bindings: NativeSlotBindingMeta[];
  isRequired: boolean;
  description?: string;
  tags?: NativeJsdocTag[];
}

interface NativeModelMeta {
  name: string;
  type: NativeTypeExpr;
}

interface NativeExposedMeta {
  name: string;
  type: NativeTypeExpr;
  description?: string;
}

interface NativeResolvedTypeMeta {
  name: string;
  type: NativeTypeExpr;
}

interface NativeComponentPropUsage {
  name: string;
  isBound: boolean;
  constness: "const" | "dynamic" | "unknown";
}

interface NativeComponentUsage {
  name: string;
  importSource?: string;
  isDynamic: boolean;
  props: NativeComponentPropUsage[];
  slotsUsed: string[];
  staticClasses: string[];
  hasDynamicClass: boolean;
  vModels: string[];
}

interface NativeTemplateRefMeta {
  name: string;
  isDynamic: boolean;
  targetTag: string;
}

interface NativeImportBindingMeta {
  name: string;
  isTypeOnly: boolean;
}

interface NativeImportMeta {
  source: string;
  isTypeOnly: boolean;
  bindings: NativeImportBindingMeta[];
}

interface NativeBindingMeta {
  name: string;
  kind: "const" | "let" | "var" | "function" | "asyncFunction" | "class";
  reactivityKind: "none" | "ref" | "reactive" | "computed" | "maybeRef" | "mutable";
  typeAnnotation?: string;
  usedInTemplate: boolean;
  usedInStyle: boolean;
}

interface NativeVueApiCallMeta {
  api: string;
  argValue?: string;
}

interface NativeSelectorMeta {
  text: string;
  specificity: [number, number, number];
}

interface NativeStyleMeta {
  lang: string;
  scoped: boolean;
  isModule: boolean;
  moduleName?: string;
  classes: string[];
  ids: string[];
  customProperties: string[];
  vBinds: string[];
  selectors: NativeSelectorMeta[];
}

interface NativeComponentFlags {
  asyncSetup: boolean;
  hasReactiveState: boolean;
  hasComputed: boolean;
  hasWatchers: boolean;
  hasLifecycleHooks: boolean;
  hasProvide: boolean;
  hasInject: boolean;
  hasInheritAttrsFalse: boolean;
  hasStoreUsage: boolean;
}

export interface NativeComponentMetaResult {
  props: NativePropMeta[];
  events: NativeEventMeta[];
  slots: NativeSlotMeta[];
  models: NativeModelMeta[];
  exposed: NativeExposedMeta[];
  typeRegistry?: NativeResolvedTypeMeta[];
  components: NativeComponentUsage[];
  templateRefs: NativeTemplateRefMeta[];
  imports: NativeImportMeta[];
  bindings: NativeBindingMeta[];
  vueApiCalls: NativeVueApiCallMeta[];
  styles: NativeStyleMeta[];
  flags: NativeComponentFlags;
  optionsApi: boolean;
  filePath: string;
}

function deriveComponentName(filePath: string): string {
  return basename(filePath).replace(/\.[^.]+$/, "") || "AnonymousComponent";
}

export function nativeComponentMetaToComponentMeta(meta: NativeComponentMetaResult): ComponentMeta {
  return {
    filePath: meta.filePath,
    componentName: deriveComponentName(meta.filePath),
    optionsApi: meta.optionsApi,
    props: meta.props.map((prop) => ({
      name: prop.name,
      type: typeExprToDescriptor(prop.type),
      required: prop.required,
      hasDefault: prop.hasDefault,
      ...(prop.rawType !== undefined ? { rawType: prop.rawType } : {}),
      ...(prop.defaultValue !== undefined ? { default: prop.defaultValue } : {}),
      ...(prop.description !== undefined ? { description: prop.description } : {}),
      ...(prop.tags?.length ? { tags: prop.tags } : {}),
    })),
    events: meta.events.map((event) => ({
      name: event.name,
      payload: typeExprToDescriptor(event.payload),
      hasValidator: false,
      isDeclared: true,
      ...(event.rawSignature !== undefined ? { rawSignature: event.rawSignature } : {}),
      ...(event.description !== undefined ? { description: event.description } : {}),
      ...(event.tags?.length ? { tags: event.tags } : {}),
    })),
    slots: meta.slots.map((slot) => ({
      name: slot.name,
      isScoped: slot.isScoped,
      bindings: slot.bindings.map((binding) => ({
        name: binding.name,
        type: typeExprToDescriptor(binding.type),
        ...(binding.rawType !== undefined ? { rawType: binding.rawType } : {}),
      })),
      isRequired: slot.isRequired,
      ...(slot.description !== undefined ? { description: slot.description } : {}),
      ...(slot.tags?.length ? { tags: slot.tags } : {}),
    })),
    models: meta.models.map((model) => ({
      name: model.name,
      type: typeExprToDescriptor(model.type),
    })),
    exposed: meta.exposed.map((exposed) => ({
      name: exposed.name,
      type: typeExprToDescriptor(exposed.type),
      ...(exposed.description !== undefined ? { description: exposed.description } : {}),
    })),
    components: meta.components.map((component) => ({
      name: component.name,
      ...(component.importSource !== undefined ? { importSource: component.importSource } : {}),
      isDynamic: component.isDynamic,
      props: component.props.map((prop) => ({
        name: prop.name,
        isBound: prop.isBound,
        constness: prop.constness,
      })),
      slotsUsed: [...component.slotsUsed],
      staticClasses: [...component.staticClasses],
      hasDynamicClass: component.hasDynamicClass,
      vModels: [...component.vModels],
    })),
    templateRefs: meta.templateRefs.map((templateRef) => ({
      name: templateRef.name,
      isDynamic: templateRef.isDynamic,
      targetTag: templateRef.targetTag,
    })),
    imports: meta.imports.map((imp) => ({
      source: imp.source,
      isTypeOnly: imp.isTypeOnly,
      bindings: imp.bindings.map((binding) => ({
        name: binding.name,
        isTypeOnly: binding.isTypeOnly,
      })),
    })),
    bindings: meta.bindings.map((binding) => ({
      name: binding.name,
      kind: binding.kind,
      reactivityKind: binding.reactivityKind,
      ...(binding.typeAnnotation !== undefined ? { typeAnnotation: binding.typeAnnotation } : {}),
      usedInTemplate: binding.usedInTemplate,
      usedInStyle: binding.usedInStyle,
    })),
    vueApiCalls: meta.vueApiCalls.map((call) => ({
      api: call.api,
      ...(call.argValue !== undefined ? { argValue: call.argValue } : {}),
    })),
    styles: meta.styles.map((style) => ({
      lang: style.lang,
      scoped: style.scoped,
      isModule: style.isModule,
      ...(style.moduleName !== undefined ? { moduleName: style.moduleName } : {}),
      classes: [...style.classes],
      ids: [...style.ids],
      customProperties: [...style.customProperties],
      vBinds: [...style.vBinds],
      selectors: style.selectors.map((selector) => ({
        text: selector.text,
        specificity: selector.specificity,
      })),
    })),
    flags: meta.flags,
  };
}

export function nativeTypeRegistryToMap(
  meta: NativeComponentMetaResult,
): Map<string, TypeDescriptor> | undefined {
  if (!meta.typeRegistry?.length) {
    return undefined;
  }
  const registry = new Map<string, TypeDescriptor>();
  for (const entry of meta.typeRegistry) {
    registry.set(entry.name, typeExprToDescriptor(entry.type));
  }
  return registry;
}
