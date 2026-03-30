import { create, toBinary } from "@bufbuild/protobuf";
import { ComponentMetaPayloadSchema, type ComponentMetaPayloadInit } from "@verter/proto";

type TestPrimitiveName =
  | "string"
  | "number"
  | "boolean"
  | "symbol"
  | "bigint"
  | "any"
  | "unknown"
  | "void"
  | "never"
  | "null"
  | "undefined"
  | "object";

export type TestTypeExpr =
  | { kind: "primitive"; name: TestPrimitiveName }
  | { kind: "literal"; literalKind: "string"; value: string }
  | { kind: "ref"; name: string }
  | { kind: "union"; types: TestTypeExpr[] }
  | { kind: "indexedAccess"; object: TestTypeExpr; index: TestTypeExpr }
  | {
      kind: "object";
      properties: Array<{ name: string; type: TestTypeExpr; optional?: boolean }>;
    };

export interface TestTypeRegistryEntry {
  name: string;
  type: TestTypeExpr;
  rawType?: string;
}

export interface TestPropMeta {
  name: string;
  type: TestTypeExpr;
  rawType?: string;
  required?: boolean;
  hasDefault?: boolean;
}

export interface TestSlotBindingMeta {
  name: string;
  type: TestTypeExpr;
  rawType?: string;
}

export interface TestSlotMeta {
  name: string;
  isScoped?: boolean;
  isRequired?: boolean;
  returnType?: string;
  bindings?: TestSlotBindingMeta[];
}

export interface TestComponentMetaPayload {
  filePath: string;
  optionsApi?: boolean;
  props?: TestPropMeta[];
  slots?: TestSlotMeta[];
  typeRegistry?: TestTypeRegistryEntry[];
}

const SCHEMA_VERSION = 1;

const NODE_PRIMITIVE = 1;
const NODE_UNION = 3;
const NODE_OBJECT = 7;
const NODE_REF = 9;

const MEMBER_PROPERTY = 1;
const ACCEPTED_SURFACE_COMPLETENESS_EXACT = 1;
const ROOT_REACHABILITY_NO_FALLTHROUGH = 1;
const FALLTHROUGH_NONE = 1;
const NO_FALLTHROUGH_REASON_NO_TEMPLATE = 5;

const primitiveTags: Record<TestPrimitiveName, number> = {
  string: 1,
  number: 2,
  boolean: 3,
  symbol: 4,
  bigint: 5,
  any: 6,
  unknown: 7,
  void: 8,
  never: 9,
  null: 10,
  undefined: 11,
  object: 12,
};

class TestGraphBuilder {
  private readonly strings: string[] = [];
  private readonly stringIds = new Map<string, number>();
  private readonly nodes: Array<{ key: string; proto: Record<string, unknown> }> = [];
  private readonly nodeIds = new Map<string, number>();

  stringId(value: string | undefined): number {
    if (!value) {
      return 0;
    }
    const existing = this.stringIds.get(value);
    if (existing !== undefined) {
      return existing;
    }
    const id = this.strings.length + 1;
    this.strings.push(value);
    this.stringIds.set(value, id);
    return id;
  }

  nodeId(expr: TestTypeExpr): number {
    const key = JSON.stringify(expr);
    const existing = this.nodeIds.get(key);
    if (existing !== undefined) {
      return existing;
    }

    let proto: Record<string, unknown>;
    switch (expr.kind) {
      case "primitive":
        proto = typeNode("primitive", { primitive: primitiveTags[expr.name] });
        break;
      case "literal":
        proto = typeNode("literal", {
          literalKind: 1,
          stringId: this.stringId(expr.value),
          numberValue: 0,
          booleanValue: false,
        });
        break;
      case "ref":
        proto = typeNode("ref", {
          nameId: this.stringId(expr.name),
          typeArgumentNodeIds: [],
        });
        break;
      case "union":
        proto = typeNode("union", {
          typeNodeIds: expr.types.map((member) => this.nodeId(member)),
        });
        break;
      case "indexedAccess":
        proto = typeNode("indexedAccess", {
          objectNodeId: this.nodeId(expr.object),
          indexNodeId: this.nodeId(expr.index),
        });
        break;
      case "object":
        proto = typeNode("object", {
          members: expr.properties.map((property) => ({
            kind: MEMBER_PROPERTY,
            nameId: this.stringId(property.name),
            typeNodeId: this.nodeId(property.type),
            optional: Boolean(property.optional),
            readonly: false,
            keyNameId: 0,
            keyTypeNodeId: 0,
            valueTypeNodeId: 0,
            functionNodeId: 0,
          })),
        });
        break;
    }

    const id = this.nodes.length + 1;
    this.nodes.push({ key, proto });
    this.nodeIds.set(key, id);
    return id;
  }

  protoStringTable(): string[] {
    return [...this.strings];
  }

  protoNodeTable(): Record<string, unknown>[] {
    return this.nodes.map((node) => node.proto);
  }
}

function typeNode(caseName: string, value: Record<string, unknown>) {
  return {
    kind: {
      case: caseName,
      value,
    },
  };
}

export function buildTestComponentMetaProtoPayload(
  input: TestComponentMetaPayload,
): ComponentMetaPayloadInit {
  const builder = new TestGraphBuilder();
  const props = input.props ?? [];
  const slots = input.slots ?? [];
  const typeRegistry = input.typeRegistry ?? [];

  const typeRegistryEntries = typeRegistry.map((entry) => ({
    nameId: builder.stringId(entry.name),
    typeNodeId: builder.nodeId(entry.type),
    rawTypeId: builder.stringId(entry.rawType),
  }));

  const body = {
    filePathId: builder.stringId(input.filePath),
    optionsApi: Boolean(input.optionsApi),
    props: props.map((prop) => ({
      nameId: builder.stringId(prop.name),
      typeNodeId: builder.nodeId(prop.type),
      rawTypeId: builder.stringId(prop.rawType),
      required: Boolean(prop.required),
      hasDefault: Boolean(prop.hasDefault),
      tags: [],
    })),
    events: [],
    slots: slots.map((slot) => ({
      nameId: builder.stringId(slot.name),
      isScoped: Boolean(slot.isScoped),
      bindings: (slot.bindings ?? []).map((binding) => ({
        nameId: builder.stringId(binding.name),
        typeNodeId: builder.nodeId(binding.type),
        rawTypeId: builder.stringId(binding.rawType),
      })),
      isRequired: Boolean(slot.isRequired),
      returnTypeId: builder.stringId(slot.returnType),
      tags: [],
    })),
    models: [],
    exposed: [],
    components: [],
    templateRefs: [],
    imports: [],
    bindings: [],
    vueApiCalls: [],
    styles: [],
    flags: {
      asyncSetup: false,
      hasReactiveState: false,
      hasComputed: false,
      hasWatchers: false,
      hasLifecycleHooks: false,
      hasProvide: false,
      hasInject: false,
      hasInheritAttrsFalse: false,
      hasStoreUsage: false,
    },
    acceptedProps: [],
    acceptedEvents: [],
    acceptedSurfaceCompleteness: ACCEPTED_SURFACE_COMPLETENESS_EXACT,
    rootReachability: {
      kind: ROOT_REACHABILITY_NO_FALLTHROUGH,
      reason: NO_FALLTHROUGH_REASON_NO_TEMPLATE,
      branches: [],
    },
    fallthroughSurface: {
      kind: FALLTHROUGH_NONE,
      reason: NO_FALLTHROUGH_REASON_NO_TEMPLATE,
      branches: [],
    },
  };

  return {
    schemaVersion: SCHEMA_VERSION,
    typeGraph: {
      strings: builder.protoStringTable(),
      nodes: builder.protoNodeTable(),
    },
    typeRegistry: typeRegistryEntries,
    body,
  };
}

export function encodeTestComponentMetaPayload(input: TestComponentMetaPayload): Buffer {
  const payload = create(ComponentMetaPayloadSchema, buildTestComponentMetaProtoPayload(input));
  return Buffer.from(toBinary(ComponentMetaPayloadSchema, payload));
}
