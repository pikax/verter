import { fromBinary } from "@bufbuild/protobuf";
import {
  ComponentMetaPayloadSchema,
  type ComponentMetaPayload,
  type ProtoRecord,
  type ProtoTypeNode,
} from "@verter/proto";

import type { NativeComponentMetaResult } from "./native-component-meta.js";
import type {
  NativeConsumedRootBindings,
  NativeRootBranch,
  NativeRootReachability,
  NativeRootTargetRef,
  NativeUnresolvedRootTargetReason,
} from "./native-component-meta.js";
import {
  BRANCH_STATUS_PARTIALLY_UNRESOLVED,
  BRANCH_STATUS_RESOLVED,
  BRANCH_STATUS_UNRESOLVED,
  createGraphTypeExprRef,
  DecodedTypeGraph,
  FALLTHROUGH_BRANCHES,
  FALLTHROUGH_NONE,
  GRAPH_FORMAT_VERSION,
  INHERITED_SOURCE_COMPONENT,
  INHERITED_SOURCE_NATIVE_TAG,
  MEMBER_AVAILABILITY_ALWAYS,
  MEMBER_AVAILABILITY_CONDITIONAL,
  MEMBER_CALL_SIGNATURE,
  MEMBER_CONSTRUCT_SIGNATURE,
  MEMBER_INDEX_SIGNATURE,
  MEMBER_METHOD,
  MEMBER_PROPERTY,
  MEMBER_PROVENANCE_DECLARED,
  MEMBER_PROVENANCE_INHERITED,
  NODE_ARRAY,
  NODE_CONDITIONAL,
  NODE_FUNCTION,
  NODE_INDEXED_ACCESS,
  NODE_INFER,
  NODE_INTERSECTION,
  NODE_KEY_OF,
  NODE_LITERAL,
  NODE_MAPPED,
  NODE_OBJECT,
  NODE_PARENTHESIZED,
  NODE_PRIMITIVE,
  NODE_REF,
  NODE_REST,
  NODE_TEMPLATE_LITERAL,
  NODE_TUPLE,
  NODE_TYPE_OF,
  NODE_TYPE_PARAMETER,
  NODE_UNION,
  NODE_UNKNOWN,
  PARTIAL_BRANCH_DYNAMIC_ATTR_NAME,
  PARTIAL_BRANCH_DYNAMIC_LISTENER_NAME,
  PARTIAL_BRANCH_GENERIC_RESOLUTION,
  PARTIAL_BRANCH_UNKNOWN_SPREAD,
  RESOLVED_ROOT_STEP_COMPONENT,
  RESOLVED_ROOT_STEP_NATIVE_TAG,
  RESOLVED_ROOT_STEP_UNRESOLVED,
  ROOT_REACHABILITY_BRANCHES,
  ROOT_REACHABILITY_NO_FALLTHROUGH,
  ROOT_TARGET_COMPONENT_USAGE,
  ROOT_TARGET_DYNAMIC_COMPONENT_USAGE,
  ROOT_TARGET_NATIVE_ELEMENT,
  ROOT_TARGET_UNRESOLVED,
  type GraphFunctionParamRecord,
  type GraphNodeRecord,
  type GraphObjectMemberRecord,
  type GraphTupleElementRecord,
  UNRESOLVED_BRANCH_CHILD_RESOLUTION_FAILED,
  UNRESOLVED_BRANCH_CYCLE,
  UNRESOLVED_BRANCH_DYNAMIC_COMPONENT_IS,
  UNRESOLVED_BRANCH_GENERIC_RESOLUTION,
  UNRESOLVED_BRANCH_ROOT_TARGET,
  UNRESOLVED_BRANCH_UNRESOLVED_CHILD_IMPORT,
  UNRESOLVED_ROOT_TARGET_DYNAMIC_COMPONENT_IS,
  UNRESOLVED_ROOT_TARGET_MISSING_USAGE_LINK,
  UNRESOLVED_ROOT_TARGET_SLOT_OUTLET,
  UNRESOLVED_ROOT_TARGET_UNKNOWN_ROOT_TARGET,
  UNRESOLVED_ROOT_TARGET_UNRESOLVED_IMPORT,
  UNRESOLVED_ROOT_TARGET_UNSUPPORTED_BUILTIN,
} from "./type-graph-core.js";

const EXPANSION_EXACTNESS_EXACT_CONCRETE = 1;
const EXPANSION_EXACTNESS_EXACT_SYMBOLIC = 2;
const EXPANSION_EXACTNESS_INCOMPLETE = 3;

const EXPANSION_EXECUTION_STATUS_COMPLETED = 1;
const EXPANSION_EXECUTION_STATUS_CANCELLED = 2;
const EXPANSION_EXECUTION_STATUS_INTERRUPTED = 3;
const EXPANSION_EXECUTION_STATUS_HARD_STOP = 4;

const EXPANSION_REASON_BUDGET_EXCEEDED = 1;
const EXPANSION_REASON_MAPPED_DEPTH_EXCEEDED = 2;
const EXPANSION_REASON_UNRESOLVED_REFERENCE = 3;
const EXPANSION_REASON_INDETERMINATE_CONDITIONAL = 4;
const EXPANSION_REASON_INFINITE_KEY_SPACE = 5;
const EXPANSION_REASON_UNSUPPORTED_OPERATOR = 6;

const ACCEPTED_SURFACE_COMPLETENESS_EXACT = 1;
const ACCEPTED_SURFACE_COMPLETENESS_LOWER_BOUND = 2;

const ACCEPTED_PROP_KIND_DECLARED_PROP = 1;
const ACCEPTED_PROP_KIND_ATTR = 2;

const ACCEPTED_EVENT_KIND_DECLARED_EMIT = 1;
const ACCEPTED_EVENT_KIND_LISTENER = 2;

const NO_FALLTHROUGH_REASON_INHERIT_ATTRS_FALSE = 1;
const NO_FALLTHROUGH_REASON_MULTI_ROOT = 2;
const NO_FALLTHROUGH_REASON_BRANCH_NOT_SINGLE_ROOT = 3;
const NO_FALLTHROUGH_REASON_ROOT_V_FOR = 4;
const NO_FALLTHROUGH_REASON_NO_TEMPLATE = 5;
const NO_FALLTHROUGH_REASON_EMPTY_TEMPLATE = 6;
const NO_FALLTHROUGH_REASON_TEXT_OR_INTERPOLATION_ROOT = 7;

const GENERIC_RESOLUTION_FAILURE_SPREAD_INPUT = 1;
const GENERIC_RESOLUTION_FAILURE_DYNAMIC_KEY = 2;
const GENERIC_RESOLUTION_FAILURE_MISSING_TYPE = 3;
const GENERIC_RESOLUTION_FAILURE_UNSUPPORTED_EXPRESSION = 4;
const GENERIC_RESOLUTION_FAILURE_MISSING_USAGE_LINK = 5;
const GENERIC_RESOLUTION_FAILURE_UNRESOLVED_CHILD_GENERIC_SURFACE = 6;

export function decodeTypedComponentMetaPayload(
  payload: ArrayBuffer | ArrayBufferView,
): NativeComponentMetaResult {
  const message = fromBinary(ComponentMetaPayloadSchema, toBytes(payload));
  return decodeTypedPayload(message);
}

function decodeTypedPayload(message: ComponentMetaPayload): NativeComponentMetaResult {
  if (message.schemaVersion !== GRAPH_FORMAT_VERSION) {
    throw graphError(
      `component-meta protobuf payload version mismatch: expected ${GRAPH_FORMAT_VERSION}, found ${message.schemaVersion}`,
    );
  }
  if (!message.typeGraph || !message.body) {
    throw graphError("component-meta protobuf payload is missing a required section");
  }

  const graph = decodeTypeGraph(message.typeGraph);
  const typeRegistry = message.typeRegistry.map((entry) => decodeTypeRegistryEntry(entry, graph));
  const body = decodeComponentMetaBody(message.body, graph);
  return {
    ...body,
    typeRegistry,
  } as unknown as NativeComponentMetaResult;
}

function decodeTypeGraph(typeGraph: ProtoRecord): DecodedTypeGraph {
  const strings = [...((typeGraph.strings as string[] | undefined) ?? [])];
  const nodes = ((typeGraph.nodes as ProtoTypeNode[] | undefined) ?? []).map(decodeTypeNode);
  const graph = new DecodedTypeGraph(strings, nodes);
  validateNodeTable(graph);
  return graph;
}

function decodeTypeNode(node: ProtoTypeNode): GraphNodeRecord {
  const kind = node.kind?.case;
  const value = node.kind?.value as ProtoRecord | undefined;

  switch (kind) {
    case "primitive":
      return {
        kind: NODE_PRIMITIVE,
        primitive: Number(value?.primitive ?? 0),
      };
    case "literal": {
      const literalKind = Number(value?.literalKind ?? 0);
      switch (literalKind) {
        case 1:
        case 4:
          return {
            kind: NODE_LITERAL,
            literalKind,
            stringId: Number(value?.stringId ?? 0),
          };
        case 2:
          return {
            kind: NODE_LITERAL,
            literalKind,
            numberValue: Number(value?.numberValue ?? 0),
          };
        case 3:
          return {
            kind: NODE_LITERAL,
            literalKind,
            booleanValue: Boolean(value?.booleanValue),
          };
        default:
          throw graphError(`component-meta graph payload has unknown literal kind ${literalKind}`);
      }
    }
    case "union":
      return { kind: NODE_UNION, typeNodeIds: numberList(value?.typeNodeIds) };
    case "intersection":
      return { kind: NODE_INTERSECTION, typeNodeIds: numberList(value?.typeNodeIds) };
    case "array":
      return {
        kind: NODE_ARRAY,
        elementNodeId: Number(value?.elementNodeId ?? 0),
        readonly: Boolean(value?.readonly),
      };
    case "tuple":
      return {
        kind: NODE_TUPLE,
        readonly: Boolean(value?.readonly),
        elements: ((value?.elements as ProtoRecord[] | undefined) ?? []).map((element) => ({
          labelId: Number(element.labelId ?? 0),
          typeNodeId: Number(element.typeNodeId ?? 0),
          optional: Boolean(element.optional),
          rest: Boolean(element.rest),
        })) as GraphTupleElementRecord[],
      };
    case "object":
      return {
        kind: NODE_OBJECT,
        members: ((value?.members as ProtoRecord[] | undefined) ?? []).map((member) => ({
          kind: Number(member.kind ?? 0),
          nameId: Number(member.nameId ?? 0),
          typeNodeId: Number(member.typeNodeId ?? 0),
          optional: Boolean(member.optional),
          readonly: Boolean(member.readonly),
          keyNameId: Number(member.keyNameId ?? 0),
          keyTypeNodeId: Number(member.keyTypeNodeId ?? 0),
          valueTypeNodeId: Number(member.valueTypeNodeId ?? 0),
          functionNodeId: Number(member.functionNodeId ?? 0),
        })) as GraphObjectMemberRecord[],
      };
    case "function":
      return {
        kind: NODE_FUNCTION,
        parameters: ((value?.parameters as ProtoRecord[] | undefined) ?? []).map((parameter) => ({
          nameId: Number(parameter.nameId ?? 0),
          typeNodeId: Number(parameter.typeNodeId ?? 0),
          optional: Boolean(parameter.optional),
          rest: Boolean(parameter.rest),
        })) as GraphFunctionParamRecord[],
        returnTypeNodeId: Number(value?.returnTypeNodeId ?? 0),
        typeParameterNodeIds: numberList(value?.typeParameterNodeIds),
      };
    case "ref":
      return {
        kind: NODE_REF,
        nameId: Number(value?.nameId ?? 0),
        typeArgumentNodeIds: numberList(value?.typeArgumentNodeIds),
      };
    case "typeParameter":
      return {
        kind: NODE_TYPE_PARAMETER,
        nameId: Number(value?.nameId ?? 0),
        constraintNodeId: Number(value?.constraintNodeId ?? 0),
        defaultNodeId: Number(value?.defaultNodeId ?? 0),
      };
    case "keyOf":
      return { kind: NODE_KEY_OF, operandNodeId: Number(value?.operandNodeId ?? 0) };
    case "typeOf":
      return { kind: NODE_TYPE_OF, pathIds: numberList(value?.pathIds) };
    case "indexedAccess":
      return {
        kind: NODE_INDEXED_ACCESS,
        objectNodeId: Number(value?.objectNodeId ?? 0),
        indexNodeId: Number(value?.indexNodeId ?? 0),
      };
    case "conditional":
      return {
        kind: NODE_CONDITIONAL,
        checkNodeId: Number(value?.checkNodeId ?? 0),
        extendsNodeId: Number(value?.extendsNodeId ?? 0),
        trueTypeNodeId: Number(value?.trueTypeNodeId ?? 0),
        falseTypeNodeId: Number(value?.falseTypeNodeId ?? 0),
      };
    case "mapped":
      return {
        kind: NODE_MAPPED,
        parameterId: Number(value?.parameterId ?? 0),
        sourceNodeId: Number(value?.sourceNodeId ?? 0),
        valueNodeId: Number(value?.valueNodeId ?? 0),
        optionalModifier: Number(value?.optionalModifier ?? 0),
        readonlyModifier: Number(value?.readonlyModifier ?? 0),
        nameTypeNodeId: Number(value?.nameTypeNodeId ?? 0),
      };
    case "templateLiteral":
      return {
        kind: NODE_TEMPLATE_LITERAL,
        quasiIds: numberList(value?.quasiIds),
        expressionNodeIds: numberList(value?.expressionNodeIds),
      };
    case "parenthesized":
      return { kind: NODE_PARENTHESIZED, innerNodeId: Number(value?.innerNodeId ?? 0) };
    case "unknown":
      return { kind: NODE_UNKNOWN, rawId: Number(value?.rawId ?? 0) };
    case "infer":
      return { kind: NODE_INFER, nameId: Number(value?.nameId ?? 0) };
    case "rest":
      return { kind: NODE_REST, innerNodeId: Number(value?.innerNodeId ?? 0) };
    default:
      throw graphError("component-meta graph payload has unknown node kind 0");
  }
}

function validateNodeTable(graph: DecodedTypeGraph): void {
  for (const node of graph.nodes) {
    switch (node.kind) {
      case NODE_LITERAL:
        if (node.stringId) {
          graph.getString(node.stringId);
        }
        break;
      case NODE_UNION:
      case NODE_INTERSECTION:
        node.typeNodeIds.forEach((id) => graph.getNode(id));
        break;
      case NODE_ARRAY:
        graph.getNode(node.elementNodeId);
        break;
      case NODE_TUPLE:
        node.elements.forEach((element) => {
          if (element.labelId) {
            graph.getString(element.labelId);
          }
          graph.getNode(element.typeNodeId);
        });
        break;
      case NODE_OBJECT:
        node.members.forEach((member) => {
          switch (member.kind) {
            case MEMBER_PROPERTY:
              if (member.nameId) {
                graph.getString(member.nameId);
              }
              graph.getNode(member.typeNodeId);
              break;
            case MEMBER_INDEX_SIGNATURE:
              if (member.keyNameId) {
                graph.getString(member.keyNameId);
              }
              graph.getNode(member.keyTypeNodeId);
              graph.getNode(member.valueTypeNodeId);
              break;
            case MEMBER_CALL_SIGNATURE:
            case MEMBER_CONSTRUCT_SIGNATURE:
            case MEMBER_METHOD:
              if (member.nameId) {
                graph.getString(member.nameId);
              }
              graph.getNode(member.functionNodeId);
              break;
            default:
              throw graphError(
                `component-meta graph payload has unknown object member kind ${member.kind}`,
              );
          }
        });
        break;
      case NODE_FUNCTION:
        node.parameters.forEach((parameter) => {
          if (parameter.nameId) {
            graph.getString(parameter.nameId);
          }
          graph.getNode(parameter.typeNodeId);
        });
        if (node.returnTypeNodeId) {
          graph.getNode(node.returnTypeNodeId);
        }
        node.typeParameterNodeIds.forEach((id) => graph.getNode(id));
        break;
      case NODE_REF:
        graph.getString(node.nameId);
        node.typeArgumentNodeIds.forEach((id) => graph.getNode(id));
        break;
      case NODE_TYPE_PARAMETER:
        graph.getString(node.nameId);
        if (node.constraintNodeId) {
          graph.getNode(node.constraintNodeId);
        }
        if (node.defaultNodeId) {
          graph.getNode(node.defaultNodeId);
        }
        break;
      case NODE_KEY_OF:
        graph.getNode(node.operandNodeId);
        break;
      case NODE_TYPE_OF:
        node.pathIds.forEach((id) => graph.getString(id));
        break;
      case NODE_INDEXED_ACCESS:
        graph.getNode(node.objectNodeId);
        graph.getNode(node.indexNodeId);
        break;
      case NODE_CONDITIONAL:
        graph.getNode(node.checkNodeId);
        graph.getNode(node.extendsNodeId);
        graph.getNode(node.trueTypeNodeId);
        graph.getNode(node.falseTypeNodeId);
        break;
      case NODE_MAPPED:
        graph.getString(node.parameterId);
        graph.getNode(node.sourceNodeId);
        graph.getNode(node.valueNodeId);
        if (node.nameTypeNodeId) {
          graph.getNode(node.nameTypeNodeId);
        }
        break;
      case NODE_TEMPLATE_LITERAL:
        node.quasiIds.forEach((id) => graph.getString(id));
        node.expressionNodeIds.forEach((id) => graph.getNode(id));
        break;
      case NODE_PARENTHESIZED:
        graph.getNode(node.innerNodeId);
        break;
      case NODE_UNKNOWN:
        graph.getString(node.rawId);
        break;
      case NODE_INFER:
        graph.getString(node.nameId);
        break;
      case NODE_REST:
        graph.getNode(node.innerNodeId);
        break;
      default:
        break;
    }
  }
}

function decodeTypeRegistryEntry(
  entry: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(entry.nameId, "registry name")),
    type: createGraphTypeExprRef(graph, readRequiredId(entry.typeNodeId, "registry type")),
    ...maybe(
      "typeExpansion",
      decodeOptionalExpansionMetadata(entry.typeExpansion as ProtoRecord | undefined, graph),
    ),
    ...maybe("rawType", graph.getStringMaybe(Number(entry.rawTypeId ?? 0))),
    ...maybe(
      "declaration",
      decodeOptionalResolvedTypeDeclaration(entry.declaration as ProtoRecord | undefined, graph),
    ),
  };
}

function decodeComponentMetaBody(
  body: ProtoRecord,
  graph: DecodedTypeGraph,
): Omit<NativeComponentMetaResult, "typeRegistry"> {
  const resolution = decodeOptionalResolution(body.resolution as ProtoRecord | undefined, graph);
  const rootReachability: NativeRootReachability = decodeRootReachability(
    requireProtoMessage(body.rootReachability as ProtoRecord | undefined, "root reachability"),
    graph,
  );
  const rootInfo =
    decodeOptionalRootInfo(body.rootInfo as ProtoRecord | undefined, graph) ??
    deriveRootInfo(rootReachability);
  return {
    filePath: graph.getString(readRequiredId(body.filePathId, "file path")),
    componentName: "",
    optionsApi: Boolean(body.optionsApi),
    props: ((body.props as ProtoRecord[] | undefined) ?? []).map((prop) => decodeProp(prop, graph)),
    events: ((body.events as ProtoRecord[] | undefined) ?? []).map((event) =>
      decodeEvent(event, graph),
    ),
    slots: ((body.slots as ProtoRecord[] | undefined) ?? []).map((slot) => decodeSlot(slot, graph)),
    models: ((body.models as ProtoRecord[] | undefined) ?? []).map((model) =>
      decodeModel(model, graph),
    ),
    exposed: ((body.exposed as ProtoRecord[] | undefined) ?? []).map((exposed) =>
      decodeExposed(exposed, graph),
    ),
    ...maybe(
      "publicInstance",
      decodeOptionalPublicInstance(body.publicInstance as ProtoRecord | undefined, graph),
    ),
    ...maybe(
      "sfcBlocks",
      decodeOptionalSfcBlocks(body.sfcBlocks as ProtoRecord | undefined, graph),
    ),
    components: ((body.components as ProtoRecord[] | undefined) ?? []).map((component) =>
      decodeComponentUsage(component, graph),
    ),
    templateRefs: ((body.templateRefs as ProtoRecord[] | undefined) ?? []).map((templateRef) =>
      decodeTemplateRef(templateRef, graph),
    ),
    imports: ((body.imports as ProtoRecord[] | undefined) ?? []).map((entry) =>
      decodeImport(entry, graph),
    ),
    bindings: ((body.bindings as ProtoRecord[] | undefined) ?? []).map((binding) =>
      decodeBinding(binding, graph),
    ),
    vueApiCalls: ((body.vueApiCalls as ProtoRecord[] | undefined) ?? []).map((call) =>
      decodeVueApiCall(call, graph),
    ),
    styles: ((body.styles as ProtoRecord[] | undefined) ?? []).map((style) =>
      decodeStyle(style, graph),
    ),
    flags: decodeComponentFlags(body.flags as ProtoRecord | undefined),
    acceptedProps: ((body.acceptedProps as ProtoRecord[] | undefined) ?? []).map((prop) =>
      decodeAcceptedProp(prop, graph),
    ),
    acceptedEvents: ((body.acceptedEvents as ProtoRecord[] | undefined) ?? []).map((event) =>
      decodeAcceptedEvent(event, graph),
    ),
    acceptedSurfaceCompleteness: decodeAcceptedSurfaceCompleteness(
      Number(body.acceptedSurfaceCompleteness ?? 0),
    ),
    ...maybe("rootInfo", rootInfo),
    rootReachability,
    fallthroughSurface: decodeFallthroughSurface(
      requireProtoMessage(
        body.fallthroughSurface as ProtoRecord | undefined,
        "fallthrough surface",
      ),
      graph,
    ),
    ...maybe("resolution", resolution),
  } as unknown as Omit<NativeComponentMetaResult, "typeRegistry">;
}

function decodeOptionalPublicInstance(
  publicInstance: ProtoRecord | undefined,
  graph: DecodedTypeGraph,
): Record<string, unknown> | undefined {
  if (!publicInstance) {
    return undefined;
  }

  return {
    completeness: Number(publicInstance.completeness ?? 0) === 1 ? "exact" : "partial",
    members: ((publicInstance.members as ProtoRecord[] | undefined) ?? []).map((member) => ({
      name: graph.getString(readRequiredId(member.nameId, "public instance member name")),
      kind:
        Number(member.kind ?? 0) === 1
          ? "prop"
          : Number(member.kind ?? 0) === 2
            ? "slotContainer"
            : "exposed",
      type: createGraphTypeExprRef(
        graph,
        readRequiredId(member.typeNodeId, "public instance member type"),
      ),
      ...maybe(
        "typeExpansion",
        decodeOptionalExpansionMetadata(member.typeExpansion as ProtoRecord | undefined, graph),
      ),
      ...maybe("rawType", graph.getStringMaybe(Number(member.rawTypeId ?? 0))),
      ...maybe("description", graph.getStringMaybe(Number(member.descriptionId ?? 0))),
    })),
  };
}

function decodeOptionalSfcBlocks(
  blocks: ProtoRecord | undefined,
  graph: DecodedTypeGraph,
): Record<string, unknown> | undefined {
  if (!blocks) {
    return undefined;
  }

  return {
    ...maybe(
      "template",
      decodeOptionalTemplateBlock(blocks.template as ProtoRecord | undefined, graph),
    ),
    ...maybe("script", decodeOptionalScriptBlock(blocks.script as ProtoRecord | undefined, graph)),
    ...maybe(
      "scriptSetup",
      decodeOptionalScriptBlock(blocks.scriptSetup as ProtoRecord | undefined, graph),
    ),
    styles: ((blocks.styles as ProtoRecord[] | undefined) ?? []).map((style) =>
      decodeStyleBlock(style, graph),
    ),
    custom: ((blocks.custom as ProtoRecord[] | undefined) ?? []).map((block) =>
      decodeCustomBlock(block, graph),
    ),
  };
}

function decodeOptionalTemplateBlock(
  block: ProtoRecord | undefined,
  graph: DecodedTypeGraph,
): Record<string, unknown> | undefined {
  if (!block) {
    return undefined;
  }
  return {
    ...maybe("lang", graph.getStringMaybe(Number(block.langId ?? 0))),
    ...maybe("src", graph.getStringMaybe(Number(block.srcId ?? 0))),
    attributes: decodeSfcAttributes((block.attributes as ProtoRecord[] | undefined) ?? [], graph),
  };
}

function decodeOptionalScriptBlock(
  block: ProtoRecord | undefined,
  graph: DecodedTypeGraph,
): Record<string, unknown> | undefined {
  if (!block) {
    return undefined;
  }
  return {
    ...maybe("lang", graph.getStringMaybe(Number(block.langId ?? 0))),
    ...maybe("src", graph.getStringMaybe(Number(block.srcId ?? 0))),
    ...maybe("generic", graph.getStringMaybe(Number(block.genericId ?? 0))),
    ...maybe("attrsType", graph.getStringMaybe(Number(block.attrsTypeId ?? 0))),
    attributes: decodeSfcAttributes((block.attributes as ProtoRecord[] | undefined) ?? [], graph),
  };
}

function decodeStyleBlock(block: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    index: Number(block.index ?? 0),
    ...maybe("lang", graph.getStringMaybe(Number(block.langId ?? 0))),
    ...maybe("src", graph.getStringMaybe(Number(block.srcId ?? 0))),
    scoped: Boolean(block.scoped),
    isModule: Boolean(block.isModule),
    ...maybe("moduleName", graph.getStringMaybe(Number(block.moduleNameId ?? 0))),
    attributes: decodeSfcAttributes((block.attributes as ProtoRecord[] | undefined) ?? [], graph),
  };
}

function decodeCustomBlock(block: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    index: Number(block.index ?? 0),
    blockType: graph.getString(readRequiredId(block.blockTypeId, "custom block type")),
    ...maybe("lang", graph.getStringMaybe(Number(block.langId ?? 0))),
    ...maybe("src", graph.getStringMaybe(Number(block.srcId ?? 0))),
    attributes: decodeSfcAttributes((block.attributes as ProtoRecord[] | undefined) ?? [], graph),
  };
}

function decodeSfcAttributes(
  attributes: ProtoRecord[],
  graph: DecodedTypeGraph,
): Record<string, unknown>[] {
  return attributes.map((attribute) => ({
    name: graph.getString(readRequiredId(attribute.nameId, "SFC attribute name")),
    ...maybe("value", graph.getStringMaybe(Number(attribute.valueId ?? 0))),
  }));
}

function decodeOptionalRootInfo(
  info: ProtoRecord | undefined,
  graph: DecodedTypeGraph,
): Record<string, unknown> | undefined {
  if (!info) {
    return undefined;
  }

  const kind = Number(info.kind ?? 0);
  const decodedKind =
    kind === 1
      ? "none"
      : kind === 2
        ? "single"
        : kind === 3
          ? "conditional"
          : kind === 4
            ? "multiple"
            : undefined;
  if (!decodedKind) {
    throw graphError(`component-meta graph payload has unknown root info kind ${kind}`);
  }

  return {
    kind: decodedKind,
    ...maybe(
      "reason",
      Number(info.reason ?? 0) === 0 ? undefined : decodeNoFallthroughReason(Number(info.reason)),
    ),
    targets: ((info.targets as ProtoRecord[] | undefined) ?? []).map((target) =>
      decodeRootTargetRef(target, graph),
    ),
  };
}

function deriveRootInfo(
  reachability: NativeComponentMetaResult["rootReachability"],
): Record<string, unknown> {
  if (reachability.kind === "branches") {
    return {
      kind: reachability.branches.length <= 1 ? "single" : "conditional",
      targets: reachability.branches.map((branch) => ({ ...branch.target })),
    };
  }

  const kind =
    reachability.reason === "multiRoot" || reachability.reason === "rootVFor"
      ? "multiple"
      : reachability.reason === "branchNotSingleRoot"
        ? "conditional"
        : "none";
  return {
    kind,
    ...(reachability.reason !== undefined ? { reason: reachability.reason } : {}),
    targets: [],
  };
}

function decodeProp(prop: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(prop.nameId, "prop name")),
    type: createGraphTypeExprRef(graph, readRequiredId(prop.typeNodeId, "prop type")),
    ...maybe(
      "typeExpansion",
      decodeOptionalExpansionMetadata(prop.typeExpansion as ProtoRecord | undefined, graph),
    ),
    ...maybe("rawType", graph.getStringMaybe(Number(prop.rawTypeId ?? 0))),
    required: Boolean(prop.required),
    hasDefault: Boolean(prop.hasDefault),
    ...maybe("defaultValue", graph.getStringMaybe(Number(prop.defaultValueId ?? 0))),
    ...maybe("description", graph.getStringMaybe(Number(prop.descriptionId ?? 0))),
    ...maybeArray("tags", decodeJsdocTags((prop.tags as ProtoRecord[] | undefined) ?? [], graph)),
  };
}

function decodeEvent(event: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(event.nameId, "event name")),
    payload: createGraphTypeExprRef(graph, readRequiredId(event.payloadNodeId, "event payload")),
    ...maybe(
      "payloadExpansion",
      decodeOptionalExpansionMetadata(event.payloadExpansion as ProtoRecord | undefined, graph),
    ),
    ...maybe("rawSignature", graph.getStringMaybe(Number(event.rawSignatureId ?? 0))),
    ...maybe("description", graph.getStringMaybe(Number(event.descriptionId ?? 0))),
    ...maybeArray("tags", decodeJsdocTags((event.tags as ProtoRecord[] | undefined) ?? [], graph)),
  };
}

function decodeSlot(slot: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(slot.nameId, "slot name")),
    isScoped: Boolean(slot.isScoped),
    bindings: ((slot.bindings as ProtoRecord[] | undefined) ?? []).map((binding) => ({
      name: graph.getString(readRequiredId(binding.nameId, "slot binding name")),
      type: createGraphTypeExprRef(graph, readRequiredId(binding.typeNodeId, "slot binding type")),
      ...maybe(
        "typeExpansion",
        decodeOptionalExpansionMetadata(binding.typeExpansion as ProtoRecord | undefined, graph),
      ),
      ...maybe("rawType", graph.getStringMaybe(Number(binding.rawTypeId ?? 0))),
    })),
    isRequired: Boolean(slot.isRequired),
    ...maybe("returnType", graph.getStringMaybe(Number(slot.returnTypeId ?? 0))),
    ...maybe("description", graph.getStringMaybe(Number(slot.descriptionId ?? 0))),
    ...maybeArray("tags", decodeJsdocTags((slot.tags as ProtoRecord[] | undefined) ?? [], graph)),
  };
}

function decodeModel(model: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(model.nameId, "model name")),
    type: createGraphTypeExprRef(graph, readRequiredId(model.typeNodeId, "model type")),
  };
}

function decodeExposed(exposed: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(exposed.nameId, "exposed name")),
    type: createGraphTypeExprRef(graph, readRequiredId(exposed.typeNodeId, "exposed type")),
    ...maybe(
      "typeExpansion",
      decodeOptionalExpansionMetadata(exposed.typeExpansion as ProtoRecord | undefined, graph),
    ),
    ...maybe("description", graph.getStringMaybe(Number(exposed.descriptionId ?? 0))),
  };
}

function decodeComponentUsage(
  component: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(component.nameId, "component usage name")),
    ...maybe("importSource", graph.getStringMaybe(Number(component.importSourceId ?? 0))),
    isDynamic: Boolean(component.isDynamic),
    props: ((component.props as ProtoRecord[] | undefined) ?? []).map((prop) => ({
      name: graph.getString(readRequiredId(prop.nameId, "component prop name")),
      isBound: Boolean(prop.isBound),
      constness: graph.getString(readRequiredId(prop.constnessId, "component prop constness")),
    })),
    slotsUsed: decodeStringList(numberList(component.slotsUsedIds), graph, "component slots used"),
    staticClasses: decodeStringList(
      numberList(component.staticClassIds),
      graph,
      "component static classes",
    ),
    hasDynamicClass: Boolean(component.hasDynamicClass),
    vModels: decodeStringList(numberList(component.vModelIds), graph, "component v-models"),
  };
}

function decodeTemplateRef(
  templateRef: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(templateRef.nameId, "template ref name")),
    isDynamic: Boolean(templateRef.isDynamic),
    targetTag: graph.getString(readRequiredId(templateRef.targetTagId, "template ref target tag")),
  };
}

function decodeImport(entry: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    source: graph.getString(readRequiredId(entry.sourceId, "import source")),
    isTypeOnly: Boolean(entry.isTypeOnly),
    bindings: ((entry.bindings as ProtoRecord[] | undefined) ?? []).map((binding) => ({
      name: graph.getString(readRequiredId(binding.nameId, "import binding name")),
      kind: graph.getString(readRequiredId(binding.kindId, "import binding kind")),
      importedName: graph.getStringMaybe(Number(binding.importedNameId ?? 0)) ?? null,
      isTypeOnly: Boolean(binding.isTypeOnly),
    })),
  };
}

function decodeBinding(binding: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(binding.nameId, "binding name")),
    kind: graph.getString(readRequiredId(binding.kindId, "binding kind")),
    reactivityKind: graph.getString(
      readRequiredId(binding.reactivityKindId, "binding reactivity kind"),
    ),
    ...maybe("typeAnnotation", graph.getStringMaybe(Number(binding.typeAnnotationId ?? 0))),
    usedInTemplate: Boolean(binding.usedInTemplate),
    usedInStyle: Boolean(binding.usedInStyle),
  };
}

function decodeVueApiCall(call: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    api: graph.getString(readRequiredId(call.apiId, "vue api name")),
    ...maybe("argValue", graph.getStringMaybe(Number(call.argValueId ?? 0))),
  };
}

function decodeStyle(style: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    lang: graph.getString(readRequiredId(style.langId, "style lang")),
    scoped: Boolean(style.scoped),
    isModule: Boolean(style.isModule),
    ...maybe("moduleName", graph.getStringMaybe(Number(style.moduleNameId ?? 0))),
    classes: decodeStringList(numberList(style.classIds), graph, "style classes"),
    ids: decodeStringList(numberList(style.idIds), graph, "style ids"),
    customProperties: decodeStringList(
      numberList(style.customPropertyIds),
      graph,
      "style custom properties",
    ),
    vBinds: decodeStringList(numberList(style.vBindIds), graph, "style v-binds"),
    selectors: ((style.selectors as ProtoRecord[] | undefined) ?? []).map((selector) => ({
      text: graph.getString(readRequiredId(selector.textId, "selector text")),
      specificity: [
        Number(selector.specificityA ?? 0),
        Number(selector.specificityB ?? 0),
        Number(selector.specificityC ?? 0),
      ] as [number, number, number],
    })),
  };
}

function decodeComponentFlags(flags: ProtoRecord | undefined): Record<string, boolean> {
  return {
    asyncSetup: Boolean(flags?.asyncSetup),
    hasReactiveState: Boolean(flags?.hasReactiveState),
    hasComputed: Boolean(flags?.hasComputed),
    hasWatchers: Boolean(flags?.hasWatchers),
    hasLifecycleHooks: Boolean(flags?.hasLifecycleHooks),
    hasProvide: Boolean(flags?.hasProvide),
    hasInject: Boolean(flags?.hasInject),
    hasInheritAttrsFalse: Boolean(flags?.hasInheritAttrsFalse),
    hasStoreUsage: Boolean(flags?.hasStoreUsage),
  };
}

function decodeAcceptedProp(prop: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(prop.nameId, "accepted prop name")),
    type: createGraphTypeExprRef(graph, readRequiredId(prop.typeNodeId, "accepted prop type")),
    ...maybe("rawType", graph.getStringMaybe(Number(prop.rawTypeId ?? 0))),
    required: Boolean(prop.required),
    provenance: decodeMemberProvenance(
      requireProtoMessage(prop.provenance as ProtoRecord | undefined, "member provenance"),
      graph,
    ),
    availability: decodeMemberAvailability(
      requireProtoMessage(prop.availability as ProtoRecord | undefined, "member availability"),
      graph,
    ),
    kind: decodeAcceptedPropKind(Number(prop.kind ?? 0)),
  };
}

function decodeAcceptedEvent(event: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(event.nameId, "accepted event name")),
    payload: createGraphTypeExprRef(
      graph,
      readRequiredId(event.payloadNodeId, "accepted event payload"),
    ),
    ...maybe("rawSignature", graph.getStringMaybe(Number(event.rawSignatureId ?? 0))),
    provenance: decodeMemberProvenance(
      requireProtoMessage(event.provenance as ProtoRecord | undefined, "member provenance"),
      graph,
    ),
    availability: decodeMemberAvailability(
      requireProtoMessage(event.availability as ProtoRecord | undefined, "member availability"),
      graph,
    ),
    kind: decodeAcceptedEventKind(Number(event.kind ?? 0)),
  };
}

function decodeMemberProvenance(
  provenance: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  const kind = Number(provenance.kind ?? 0);
  switch (kind) {
    case MEMBER_PROVENANCE_DECLARED:
      return { kind: "declared" };
    case MEMBER_PROVENANCE_INHERITED:
      return {
        kind: "inherited",
        sources: decodeInheritedSources(
          (provenance.sources as ProtoRecord[] | undefined) ?? [],
          graph,
        ),
      };
    default:
      throw graphError(`component-meta graph payload has unknown member provenance kind ${kind}`);
  }
}

function decodeInheritedSources(
  sources: ProtoRecord[],
  graph: DecodedTypeGraph,
): Array<Record<string, unknown>> {
  return sources.map((source) => {
    const kind = Number(source.kind ?? 0);
    switch (kind) {
      case INHERITED_SOURCE_NATIVE_TAG:
        return {
          kind: "nativeTag",
          tag: graph.getString(readRequiredId(source.tagId, "inherited native tag")),
        };
      case INHERITED_SOURCE_COMPONENT:
        return {
          kind: "component",
          canonicalId: graph.getString(
            readRequiredId(source.canonicalIdId, "inherited component canonical id"),
          ),
        };
      default:
        throw graphError(`component-meta graph payload has unknown inherited source kind ${kind}`);
    }
  });
}

function decodeMemberAvailability(
  availability: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  const kind = Number(availability.kind ?? 0);
  switch (kind) {
    case MEMBER_AVAILABILITY_ALWAYS:
      return { kind: "always" };
    case MEMBER_AVAILABILITY_CONDITIONAL:
      return {
        kind: "conditional",
        branchKeys: decodeStringList(numberList(availability.branchKeyIds), graph, "branch keys"),
      };
    default:
      throw graphError(`component-meta graph payload has unknown member availability kind ${kind}`);
  }
}

function decodeRootReachability(
  reachability: ProtoRecord,
  graph: DecodedTypeGraph,
): NativeRootReachability {
  const kind = Number(reachability.kind ?? 0);
  switch (kind) {
    case ROOT_REACHABILITY_NO_FALLTHROUGH:
      return {
        kind: "noFallthrough",
        reason: decodeNoFallthroughReason(Number(reachability.reason ?? 0)),
      };
    case ROOT_REACHABILITY_BRANCHES:
      return {
        kind: "branches",
        branches: ((reachability.branches as ProtoRecord[] | undefined) ?? []).map((branch) =>
          decodeRootBranch(branch, graph),
        ),
      };
    default:
      throw graphError(`component-meta graph payload has unknown root reachability kind ${kind}`);
  }
}

function decodeRootBranch(branch: ProtoRecord, graph: DecodedTypeGraph): NativeRootBranch {
  return {
    branchIndex: Number(branch.branchIndex ?? 0),
    ...maybe("conditionText", graph.getStringMaybe(Number(branch.conditionTextId ?? 0))),
    target: decodeRootTargetRef(
      requireProtoMessage(branch.target as ProtoRecord | undefined, "root branch target"),
      graph,
    ),
    consumed: decodeConsumedRootBindings(
      requireProtoMessage(
        branch.consumed as ProtoRecord | undefined,
        "root branch consumed bindings",
      ),
      graph,
    ),
    hasUnknownSpread: Boolean(branch.hasUnknownSpread),
  };
}

function decodeRootTargetRef(
  target: ProtoRecord,
  graph: DecodedTypeGraph,
): NativeRootTargetRef {
  const kind = Number(target.kind ?? 0);
  switch (kind) {
    case ROOT_TARGET_NATIVE_ELEMENT:
      return {
        kind: "nativeElement",
        elementIndex: Number(target.elementIndex ?? 0),
        tag: graph.getString(readRequiredId(target.tagId, "root target native tag")),
      };
    case ROOT_TARGET_DYNAMIC_COMPONENT_USAGE:
      return {
        kind: "dynamicComponentUsage",
        elementIndex: Number(target.elementIndex ?? 0),
        usageIndex: Number(target.usageIndex ?? 0),
      };
    case ROOT_TARGET_COMPONENT_USAGE:
      return {
        kind: "componentUsage",
        elementIndex: Number(target.elementIndex ?? 0),
        usageIndex: Number(target.usageIndex ?? 0),
        name: graph.getString(readRequiredId(target.nameId, "root target component name")),
        ...maybe("importSource", graph.getStringMaybe(Number(target.importSourceId ?? 0))),
      };
    case ROOT_TARGET_UNRESOLVED:
      return {
        kind: "unresolvedTarget",
        elementIndex: Number(target.elementIndex ?? 0),
        tag: graph.getString(readRequiredId(target.tagId, "root target unresolved tag")),
        reason: decodeUnresolvedRootTargetReason(
          requireProtoMessage(
            target.unresolvedReason as ProtoRecord | undefined,
            "unresolved root target reason",
          ),
          graph,
        ),
      };
    default:
      throw graphError(`component-meta graph payload has unknown root target kind ${kind}`);
  }
}

function decodeUnresolvedRootTargetReason(
  reason: ProtoRecord,
  graph: DecodedTypeGraph,
): NativeUnresolvedRootTargetReason {
  const kind = Number(reason.kind ?? 0);
  switch (kind) {
    case UNRESOLVED_ROOT_TARGET_DYNAMIC_COMPONENT_IS:
      return { kind: "dynamicComponentIs" };
    case UNRESOLVED_ROOT_TARGET_SLOT_OUTLET:
      return { kind: "slotOutlet" };
    case UNRESOLVED_ROOT_TARGET_UNSUPPORTED_BUILTIN:
      return {
        kind: "unsupportedBuiltin",
        tag: graph.getString(readRequiredId(reason.tagId, "unsupported builtin tag")),
      };
    case UNRESOLVED_ROOT_TARGET_MISSING_USAGE_LINK:
      return { kind: "missingUsageLink" };
    case UNRESOLVED_ROOT_TARGET_UNRESOLVED_IMPORT:
      return { kind: "unresolvedImport" };
    case UNRESOLVED_ROOT_TARGET_UNKNOWN_ROOT_TARGET:
      return { kind: "unknownRootTarget" };
    default:
      throw graphError(
        `component-meta graph payload has unknown unresolved root target reason ${kind}`,
      );
  }
}

function decodeConsumedRootBindings(
  consumed: ProtoRecord,
  graph: DecodedTypeGraph,
): NativeConsumedRootBindings {
  return {
    attrs: decodeStringList(numberList(consumed.attrIds), graph, "consumed attrs"),
    listeners: decodeStringList(numberList(consumed.listenerIds), graph, "consumed listeners"),
    hasDynamicAttrName: Boolean(consumed.hasDynamicAttrName),
    hasDynamicListenerName: Boolean(consumed.hasDynamicListenerName),
  };
}

function decodeFallthroughSurface(
  surface: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  const kind = Number(surface.kind ?? 0);
  switch (kind) {
    case FALLTHROUGH_NONE:
      return {
        kind: "none",
        reason: decodeNoFallthroughReason(Number(surface.reason ?? 0)),
      };
    case FALLTHROUGH_BRANCHES:
      return {
        kind: "branches",
        branches: ((surface.branches as ProtoRecord[] | undefined) ?? []).map((branch) =>
          decodeFallthroughBranch(branch, graph),
        ),
      };
    default:
      throw graphError(`component-meta graph payload has unknown fallthrough surface kind ${kind}`);
  }
}

function decodeFallthroughBranch(
  branch: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  return {
    branchKey: graph.getString(readRequiredId(branch.branchKeyId, "fallthrough branch key")),
    ...maybe("conditionText", graph.getStringMaybe(Number(branch.conditionTextId ?? 0))),
    props: ((branch.props as ProtoRecord[] | undefined) ?? []).map((prop) => ({
      name: graph.getString(readRequiredId(prop.nameId, "fallthrough prop name")),
      type: createGraphTypeExprRef(graph, readRequiredId(prop.typeNodeId, "fallthrough prop type")),
      ...maybe("rawType", graph.getStringMaybe(Number(prop.rawTypeId ?? 0))),
      sources: decodeInheritedSources((prop.sources as ProtoRecord[] | undefined) ?? [], graph),
    })),
    events: ((branch.events as ProtoRecord[] | undefined) ?? []).map((event) => ({
      name: graph.getString(readRequiredId(event.nameId, "fallthrough event name")),
      payload: createGraphTypeExprRef(
        graph,
        readRequiredId(event.payloadNodeId, "fallthrough event payload"),
      ),
      ...maybe("rawSignature", graph.getStringMaybe(Number(event.rawSignatureId ?? 0))),
      sources: decodeInheritedSources((event.sources as ProtoRecord[] | undefined) ?? [], graph),
    })),
    rootChain: ((branch.rootChain as ProtoRecord[] | undefined) ?? []).map((step) =>
      decodeResolvedRootStep(step, graph),
    ),
    status: decodeBranchStatus(
      requireProtoMessage(branch.status as ProtoRecord | undefined, "branch status"),
      graph,
    ),
  };
}

function decodeResolvedRootStep(
  step: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  const kind = Number(step.kind ?? 0);
  switch (kind) {
    case RESOLVED_ROOT_STEP_NATIVE_TAG:
      return {
        kind: "nativeTag",
        tag: graph.getString(readRequiredId(step.tagId, "resolved root native tag")),
      };
    case RESOLVED_ROOT_STEP_COMPONENT:
      return {
        kind: "component",
        canonicalId: graph.getString(
          readRequiredId(step.canonicalIdId, "resolved root component canonical id"),
        ),
        componentName: graph.getString(
          readRequiredId(step.componentNameId, "resolved root component name"),
        ),
      };
    case RESOLVED_ROOT_STEP_UNRESOLVED:
      return {
        kind: "unresolved",
        tag: graph.getString(readRequiredId(step.tagId, "resolved root unresolved tag")),
        reason: decodeUnresolvedBranchReason(
          requireProtoMessage(
            step.reason as ProtoRecord | undefined,
            "resolved root unresolved reason",
          ),
          graph,
        ),
      };
    default:
      throw graphError(`component-meta graph payload has unknown resolved root step kind ${kind}`);
  }
}

function decodeBranchStatus(status: ProtoRecord, graph: DecodedTypeGraph): Record<string, unknown> {
  const kind = Number(status.kind ?? 0);
  switch (kind) {
    case BRANCH_STATUS_RESOLVED:
      return { kind: "resolved" };
    case BRANCH_STATUS_PARTIALLY_UNRESOLVED:
      return {
        kind: "partiallyUnresolved",
        reasons: ((status.reasons as ProtoRecord[] | undefined) ?? []).map((reason) =>
          decodePartialBranchReason(reason),
        ),
      };
    case BRANCH_STATUS_UNRESOLVED:
      return {
        kind: "unresolved",
        reason: decodeUnresolvedBranchReason(
          requireProtoMessage(status.reason as ProtoRecord | undefined, "unresolved branch reason"),
          graph,
        ),
      };
    default:
      throw graphError(`component-meta graph payload has unknown branch status kind ${kind}`);
  }
}

function decodePartialBranchReason(reason: ProtoRecord): Record<string, unknown> {
  const kind = Number(reason.kind ?? 0);
  switch (kind) {
    case PARTIAL_BRANCH_DYNAMIC_ATTR_NAME:
      return { kind: "dynamicAttrName" };
    case PARTIAL_BRANCH_DYNAMIC_LISTENER_NAME:
      return { kind: "dynamicListenerName" };
    case PARTIAL_BRANCH_UNKNOWN_SPREAD:
      return { kind: "unknownSpread" };
    case PARTIAL_BRANCH_GENERIC_RESOLUTION:
      return {
        kind: "genericResolution",
        failure: decodeGenericResolutionFailure(Number(reason.failure ?? 0)),
      };
    default:
      throw graphError(
        `component-meta graph payload has unknown partial branch reason kind ${kind}`,
      );
  }
}

function decodeUnresolvedBranchReason(
  reason: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  const kind = Number(reason.kind ?? 0);
  switch (kind) {
    case UNRESOLVED_BRANCH_CYCLE:
      return {
        kind: "cycle",
        canonicalId: graph.getString(
          readRequiredId(reason.canonicalIdId, "unresolved branch cycle canonical id"),
        ),
      };
    case UNRESOLVED_BRANCH_DYNAMIC_COMPONENT_IS:
      return { kind: "dynamicComponentIs" };
    case UNRESOLVED_BRANCH_CHILD_RESOLUTION_FAILED:
      return { kind: "childResolutionFailed" };
    case UNRESOLVED_BRANCH_UNRESOLVED_CHILD_IMPORT:
      return {
        kind: "unresolvedChildImport",
        ...maybe("importSource", graph.getStringMaybe(Number(reason.importSourceId ?? 0))),
      };
    case UNRESOLVED_BRANCH_ROOT_TARGET:
      return {
        kind: "rootTarget",
        reason: decodeUnresolvedRootTargetReason(
          requireProtoMessage(
            reason.rootTargetReason as ProtoRecord | undefined,
            "root target reason",
          ),
          graph,
        ),
      };
    case UNRESOLVED_BRANCH_GENERIC_RESOLUTION:
      return {
        kind: "genericResolution",
        failure: decodeGenericResolutionFailure(Number(reason.failure ?? 0)),
      };
    default:
      throw graphError(
        `component-meta graph payload has unknown unresolved branch reason kind ${kind}`,
      );
  }
}

function decodeOptionalResolution(
  resolution: ProtoRecord | undefined,
  graph: DecodedTypeGraph,
): Record<string, unknown> | undefined {
  if (!resolution) {
    return undefined;
  }
  return {
    mode: graph.getString(readRequiredId(resolution.modeId, "component resolution mode")),
    macros: ((resolution.macros as ProtoRecord[] | undefined) ?? []).map((macro) =>
      decodeResolvedMacroMeta(macro, graph),
    ),
  };
}

function decodeResolvedMacroMeta(
  macro: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  return {
    macroIndex: Number(macro.macroIndex ?? 0),
    macroKind: graph.getString(readRequiredId(macro.macroKindId, "resolved macro kind")),
    typeName: graph.getString(readRequiredId(macro.typeNameId, "resolved macro type name")),
    importSource: graph.getString(
      readRequiredId(macro.importSourceId, "resolved macro import source"),
    ),
    declaration: decodeResolvedTypeDeclaration(
      requireProtoMessage(
        macro.declaration as ProtoRecord | undefined,
        "resolved macro declaration",
      ),
      graph,
    ),
    ...maybeArray(
      "nativeProps",
      ((macro.nativeProps as ProtoRecord[] | undefined) ?? []).map((prop) =>
        decodeResolvedNativeProp(prop, graph),
      ),
    ),
    ...maybeArray(
      "props",
      ((macro.props as ProtoRecord[] | undefined) ?? []).map((prop) =>
        decodeResolvedPropField(prop, graph),
      ),
    ),
    ...maybeArray(
      "emits",
      ((macro.emits as ProtoRecord[] | undefined) ?? []).map((emit) =>
        decodeResolvedEmitField(emit, graph),
      ),
    ),
    ...maybeArray(
      "slots",
      ((macro.slots as ProtoRecord[] | undefined) ?? []).map((slot) =>
        decodeResolvedSlotField(slot, graph),
      ),
    ),
    ...maybe("jsdoc", decodeOptionalJsdocBlock(macro.jsdoc as ProtoRecord | undefined, graph)),
  };
}

function decodeResolvedNativeProp(
  prop: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(prop.nameId, "resolved native prop name")),
    isOptional: Boolean(prop.isOptional),
    ...maybe("typeAnnotation", graph.getStringMaybe(Number(prop.typeAnnotationId ?? 0))),
    visibility: graph.getString(
      readRequiredId(prop.visibilityId, "resolved native prop visibility"),
    ),
    spanStart: Number(prop.spanStart ?? 0),
    spanEnd: Number(prop.spanEnd ?? 0),
  };
}

function decodeResolvedPropField(
  prop: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(prop.nameId, "resolved prop name")),
    isOptional: Boolean(prop.isOptional),
    ...maybe("typeAnnotation", graph.getStringMaybe(Number(prop.typeAnnotationId ?? 0))),
    ...maybe("description", graph.getStringMaybe(Number(prop.descriptionId ?? 0))),
    ...maybeArray("tags", decodeJsdocTags((prop.tags as ProtoRecord[] | undefined) ?? [], graph)),
  };
}

function decodeResolvedEmitField(
  emit: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(emit.nameId, "resolved emit name")),
    ...maybe("payloadType", graph.getStringMaybe(Number(emit.payloadTypeId ?? 0))),
    ...maybe("description", graph.getStringMaybe(Number(emit.descriptionId ?? 0))),
    ...maybeArray("tags", decodeJsdocTags((emit.tags as ProtoRecord[] | undefined) ?? [], graph)),
  };
}

function decodeResolvedSlotField(
  slot: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(slot.nameId, "resolved slot name")),
    isRequired: Boolean(slot.isRequired),
    bindings: ((slot.bindings as ProtoRecord[] | undefined) ?? []).map((binding) => ({
      name: graph.getString(readRequiredId(binding.nameId, "resolved slot binding name")),
      ...maybe("typeAnnotation", graph.getStringMaybe(Number(binding.typeAnnotationId ?? 0))),
    })),
    ...maybe("returnType", graph.getStringMaybe(Number(slot.returnTypeId ?? 0))),
    ...maybe("description", graph.getStringMaybe(Number(slot.descriptionId ?? 0))),
    ...maybeArray("tags", decodeJsdocTags((slot.tags as ProtoRecord[] | undefined) ?? [], graph)),
  };
}

function decodeOptionalJsdocBlock(
  jsdoc: ProtoRecord | undefined,
  graph: DecodedTypeGraph,
): Record<string, unknown> | undefined {
  if (!jsdoc) {
    return undefined;
  }
  return {
    ...maybe("description", graph.getStringMaybe(Number(jsdoc.descriptionId ?? 0))),
    ...maybeArray(
      "tags",
      ((jsdoc.tags as ProtoRecord[] | undefined) ?? []).map((tag) =>
        decodeResolvedJsdocTag(tag, graph),
      ),
    ),
  };
}

function decodeResolvedJsdocTag(
  tag: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  return {
    name: graph.getString(readRequiredId(tag.nameId, "resolved jsdoc tag name")),
    ...maybe("text", graph.getStringMaybe(Number(tag.textId ?? 0))),
    ...maybe("rawType", graph.getStringMaybe(Number(tag.rawTypeId ?? 0))),
    ...maybe("subjectName", graph.getStringMaybe(Number(tag.subjectNameId ?? 0))),
    ...maybe("resolvedType", createOptionalGraphRef(graph, Number(tag.resolvedTypeNodeId ?? 0))),
  };
}

function decodeJsdocTags(
  tags: ProtoRecord[],
  graph: DecodedTypeGraph,
): Array<Record<string, unknown>> {
  return tags.map((tag) => ({
    name: graph.getString(readRequiredId(tag.nameId, "jsdoc tag name")),
    ...maybe("text", graph.getStringMaybe(Number(tag.textId ?? 0))),
  }));
}

function decodeOptionalExpansionMetadata(
  metadata: ProtoRecord | undefined,
  graph: DecodedTypeGraph,
): Record<string, unknown> | undefined {
  if (!metadata) {
    return undefined;
  }
  return {
    exactness: decodeExpansionExactness(Number(metadata.exactness ?? 0)),
    executionStatus: decodeExpansionExecutionStatus(Number(metadata.executionStatus ?? 0)),
    diagnostics: ((metadata.diagnostics as ProtoRecord[] | undefined) ?? []).map((diagnostic) => ({
      reason: decodeExpansionStopReason(Number(diagnostic.reason ?? 0)),
      context: graph.getString(
        readRequiredId(diagnostic.contextId, "expansion diagnostic context"),
      ),
      ...maybe("propertyName", graph.getStringMaybe(Number(diagnostic.propertyNameId ?? 0))),
    })),
  };
}

function decodeOptionalResolvedTypeDeclaration(
  declaration: ProtoRecord | undefined,
  graph: DecodedTypeGraph,
): Record<string, unknown> | undefined {
  return declaration ? decodeResolvedTypeDeclaration(declaration, graph) : undefined;
}

function decodeResolvedTypeDeclaration(
  declaration: ProtoRecord,
  graph: DecodedTypeGraph,
): Record<string, unknown> {
  return {
    requestedName: graph.getString(
      readRequiredId(declaration.requestedNameId, "resolved declaration requested name"),
    ),
    resolvedName: graph.getString(
      readRequiredId(declaration.resolvedNameId, "resolved declaration resolved name"),
    ),
    canonicalSource: graph.getString(
      readRequiredId(declaration.canonicalSourceId, "resolved declaration canonical source"),
    ),
    spanStart: Number(declaration.spanStart ?? 0),
    spanEnd: Number(declaration.spanEnd ?? 0),
    kind: graph.getString(readRequiredId(declaration.kindId, "resolved declaration kind")),
    ...maybe("text", graph.getStringMaybe(Number(declaration.textId ?? 0))),
  };
}

function decodeStringList(ids: number[], graph: DecodedTypeGraph, context: string): string[] {
  return ids.map((id) => graph.getString(readRequiredId(id, context)));
}

function decodeExpansionExactness(
  value: number,
): "exactConcrete" | "exactSymbolic" | "incomplete" {
  switch (value) {
    case EXPANSION_EXACTNESS_EXACT_CONCRETE:
      return "exactConcrete";
    case EXPANSION_EXACTNESS_EXACT_SYMBOLIC:
      return "exactSymbolic";
    case EXPANSION_EXACTNESS_INCOMPLETE:
      return "incomplete";
    default:
      throw graphError(`component-meta graph payload has unknown expansion exactness ${value}`);
  }
}

function decodeExpansionExecutionStatus(
  value: number,
): "completed" | "cancelled" | "interrupted" | "hardStop" {
  switch (value) {
    case 0:
      return "completed";
    case EXPANSION_EXECUTION_STATUS_COMPLETED:
      return "completed";
    case EXPANSION_EXECUTION_STATUS_CANCELLED:
      return "cancelled";
    case EXPANSION_EXECUTION_STATUS_INTERRUPTED:
      return "interrupted";
    case EXPANSION_EXECUTION_STATUS_HARD_STOP:
      return "hardStop";
    default:
      throw graphError(
        `component-meta graph payload has unknown expansion execution status ${value}`,
      );
  }
}

function decodeExpansionStopReason(
  value: number,
):
  | "budgetExceeded"
  | "mappedDepthExceeded"
  | "unresolvedReference"
  | "indeterminateConditional"
  | "infiniteKeySpace"
  | "unsupportedOperator" {
  switch (value) {
    case EXPANSION_REASON_BUDGET_EXCEEDED:
      return "budgetExceeded";
    case EXPANSION_REASON_MAPPED_DEPTH_EXCEEDED:
      return "mappedDepthExceeded";
    case EXPANSION_REASON_UNRESOLVED_REFERENCE:
      return "unresolvedReference";
    case EXPANSION_REASON_INDETERMINATE_CONDITIONAL:
      return "indeterminateConditional";
    case EXPANSION_REASON_INFINITE_KEY_SPACE:
      return "infiniteKeySpace";
    case EXPANSION_REASON_UNSUPPORTED_OPERATOR:
      return "unsupportedOperator";
    default:
      throw graphError(`component-meta graph payload has unknown expansion stop reason ${value}`);
  }
}

function decodeAcceptedSurfaceCompleteness(value: number): "exact" | "lowerBound" {
  switch (value) {
    case ACCEPTED_SURFACE_COMPLETENESS_EXACT:
      return "exact";
    case ACCEPTED_SURFACE_COMPLETENESS_LOWER_BOUND:
      return "lowerBound";
    default:
      throw graphError(
        `component-meta graph payload has unknown accepted surface completeness ${value}`,
      );
  }
}

function decodeAcceptedPropKind(value: number): "declaredProp" | "attr" {
  switch (value) {
    case ACCEPTED_PROP_KIND_DECLARED_PROP:
      return "declaredProp";
    case ACCEPTED_PROP_KIND_ATTR:
      return "attr";
    default:
      throw graphError(`component-meta graph payload has unknown accepted prop kind ${value}`);
  }
}

function decodeAcceptedEventKind(value: number): "declaredEmit" | "listener" {
  switch (value) {
    case ACCEPTED_EVENT_KIND_DECLARED_EMIT:
      return "declaredEmit";
    case ACCEPTED_EVENT_KIND_LISTENER:
      return "listener";
    default:
      throw graphError(`component-meta graph payload has unknown accepted event kind ${value}`);
  }
}

function decodeNoFallthroughReason(
  value: number,
):
  | "inheritAttrsFalse"
  | "multiRoot"
  | "branchNotSingleRoot"
  | "rootVFor"
  | "noTemplate"
  | "emptyTemplate"
  | "textOrInterpolationRoot" {
  switch (value) {
    case NO_FALLTHROUGH_REASON_INHERIT_ATTRS_FALSE:
      return "inheritAttrsFalse";
    case NO_FALLTHROUGH_REASON_MULTI_ROOT:
      return "multiRoot";
    case NO_FALLTHROUGH_REASON_BRANCH_NOT_SINGLE_ROOT:
      return "branchNotSingleRoot";
    case NO_FALLTHROUGH_REASON_ROOT_V_FOR:
      return "rootVFor";
    case NO_FALLTHROUGH_REASON_NO_TEMPLATE:
      return "noTemplate";
    case NO_FALLTHROUGH_REASON_EMPTY_TEMPLATE:
      return "emptyTemplate";
    case NO_FALLTHROUGH_REASON_TEXT_OR_INTERPOLATION_ROOT:
      return "textOrInterpolationRoot";
    default:
      throw graphError(`component-meta graph payload has unknown no-fallthrough reason ${value}`);
  }
}

function decodeGenericResolutionFailure(
  value: number,
):
  | "spreadInput"
  | "dynamicKey"
  | "missingType"
  | "unsupportedExpression"
  | "missingUsageLink"
  | "unresolvedChildGenericSurface" {
  switch (value) {
    case GENERIC_RESOLUTION_FAILURE_SPREAD_INPUT:
      return "spreadInput";
    case GENERIC_RESOLUTION_FAILURE_DYNAMIC_KEY:
      return "dynamicKey";
    case GENERIC_RESOLUTION_FAILURE_MISSING_TYPE:
      return "missingType";
    case GENERIC_RESOLUTION_FAILURE_UNSUPPORTED_EXPRESSION:
      return "unsupportedExpression";
    case GENERIC_RESOLUTION_FAILURE_MISSING_USAGE_LINK:
      return "missingUsageLink";
    case GENERIC_RESOLUTION_FAILURE_UNRESOLVED_CHILD_GENERIC_SURFACE:
      return "unresolvedChildGenericSurface";
    default:
      throw graphError(
        `component-meta graph payload has unknown generic resolution failure ${value}`,
      );
  }
}

function createOptionalGraphRef(graph: DecodedTypeGraph, nodeId: number) {
  if (nodeId === 0) {
    return undefined;
  }
  return createGraphTypeExprRef(graph, nodeId);
}

function maybe<T>(key: string, value: T | undefined) {
  return value === undefined ? {} : { [key]: value };
}

function maybeArray<T>(key: string, value: T[]) {
  return value.length === 0 ? {} : { [key]: value };
}

function readRequiredId(value: unknown, context: string): number {
  const id = Number(value ?? 0);
  if (id <= 0) {
    throw graphError(`component-meta graph missing required id while reading ${context}`);
  }
  return id;
}

function requireProtoMessage<T extends ProtoRecord>(value: T | undefined, context: string): T {
  if (!value) {
    throw graphError(`component-meta protobuf payload is missing ${context}`);
  }
  return value;
}

function numberList(value: unknown): number[] {
  return Array.isArray(value) ? value.map((entry) => Number(entry ?? 0)) : [];
}

function toBytes(payload: ArrayBuffer | ArrayBufferView): Uint8Array {
  if (payload instanceof Uint8Array) {
    return payload;
  }
  if (ArrayBuffer.isView(payload)) {
    return new Uint8Array(payload.buffer, payload.byteOffset, payload.byteLength);
  }
  return new Uint8Array(payload);
}

function graphError(message: string): Error {
  return new Error(message);
}
