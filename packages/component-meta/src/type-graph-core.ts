import type { TypeDescriptor } from "./type-ir.js";

export const GRAPH_FORMAT_VERSION = 1;

export const NODE_PRIMITIVE = 1;
export const NODE_LITERAL = 2;
export const NODE_UNION = 3;
export const NODE_INTERSECTION = 4;
export const NODE_ARRAY = 5;
export const NODE_TUPLE = 6;
export const NODE_OBJECT = 7;
export const NODE_FUNCTION = 8;
export const NODE_REF = 9;
export const NODE_TYPE_PARAMETER = 10;
export const NODE_KEY_OF = 11;
export const NODE_TYPE_OF = 12;
export const NODE_INDEXED_ACCESS = 13;
export const NODE_CONDITIONAL = 14;
export const NODE_MAPPED = 15;
export const NODE_TEMPLATE_LITERAL = 16;
export const NODE_PARENTHESIZED = 17;
export const NODE_UNKNOWN = 18;
export const NODE_INFER = 19;
export const NODE_REST = 20;

export const LITERAL_STRING = 1;
export const LITERAL_NUMBER = 2;
export const LITERAL_BOOLEAN = 3;
export const LITERAL_BIG_INT = 4;

export const MEMBER_PROPERTY = 1;
export const MEMBER_INDEX_SIGNATURE = 2;
export const MEMBER_CALL_SIGNATURE = 3;
export const MEMBER_CONSTRUCT_SIGNATURE = 4;
export const MEMBER_METHOD = 5;

export const ROOT_REACHABILITY_NO_FALLTHROUGH = 1;
export const ROOT_REACHABILITY_BRANCHES = 2;

export const ROOT_TARGET_NATIVE_ELEMENT = 1;
export const ROOT_TARGET_DYNAMIC_COMPONENT_USAGE = 2;
export const ROOT_TARGET_COMPONENT_USAGE = 3;
export const ROOT_TARGET_UNRESOLVED = 4;

export const FALLTHROUGH_NONE = 1;
export const FALLTHROUGH_BRANCHES = 2;

export const BRANCH_STATUS_RESOLVED = 1;
export const BRANCH_STATUS_PARTIALLY_UNRESOLVED = 2;
export const BRANCH_STATUS_UNRESOLVED = 3;

export const RESOLVED_ROOT_STEP_NATIVE_TAG = 1;
export const RESOLVED_ROOT_STEP_COMPONENT = 2;
export const RESOLVED_ROOT_STEP_UNRESOLVED = 3;

export const UNRESOLVED_ROOT_TARGET_DYNAMIC_COMPONENT_IS = 1;
export const UNRESOLVED_ROOT_TARGET_SLOT_OUTLET = 2;
export const UNRESOLVED_ROOT_TARGET_UNSUPPORTED_BUILTIN = 3;
export const UNRESOLVED_ROOT_TARGET_MISSING_USAGE_LINK = 4;
export const UNRESOLVED_ROOT_TARGET_UNRESOLVED_IMPORT = 5;
export const UNRESOLVED_ROOT_TARGET_UNKNOWN_ROOT_TARGET = 6;

export const PARTIAL_BRANCH_DYNAMIC_ATTR_NAME = 1;
export const PARTIAL_BRANCH_DYNAMIC_LISTENER_NAME = 2;
export const PARTIAL_BRANCH_UNKNOWN_SPREAD = 3;
export const PARTIAL_BRANCH_GENERIC_RESOLUTION = 4;

export const UNRESOLVED_BRANCH_CYCLE = 1;
export const UNRESOLVED_BRANCH_DYNAMIC_COMPONENT_IS = 2;
export const UNRESOLVED_BRANCH_CHILD_RESOLUTION_FAILED = 3;
export const UNRESOLVED_BRANCH_UNRESOLVED_CHILD_IMPORT = 4;
export const UNRESOLVED_BRANCH_ROOT_TARGET = 5;
export const UNRESOLVED_BRANCH_GENERIC_RESOLUTION = 6;

export const MEMBER_PROVENANCE_DECLARED = 1;
export const MEMBER_PROVENANCE_INHERITED = 2;

export const INHERITED_SOURCE_NATIVE_TAG = 1;
export const INHERITED_SOURCE_COMPONENT = 2;

export const MEMBER_AVAILABILITY_ALWAYS = 1;
export const MEMBER_AVAILABILITY_CONDITIONAL = 2;

const GRAPH_TYPE_REF = Symbol("verter.component-meta.graph-type-ref");

export interface GraphTupleElementRecord {
  labelId: number;
  typeNodeId: number;
  optional: boolean;
  rest: boolean;
}

export interface GraphObjectMemberRecord {
  kind: number;
  nameId: number;
  typeNodeId: number;
  optional: boolean;
  readonly: boolean;
  keyNameId: number;
  keyTypeNodeId: number;
  valueTypeNodeId: number;
  functionNodeId: number;
}

export interface GraphFunctionParamRecord {
  nameId: number;
  typeNodeId: number;
  optional: boolean;
  rest: boolean;
}

export type GraphNodeRecord =
  | { kind: typeof NODE_PRIMITIVE; primitive: number }
  | {
      kind: typeof NODE_LITERAL;
      literalKind: number;
      stringId?: number;
      numberValue?: number;
      booleanValue?: boolean;
    }
  | { kind: typeof NODE_UNION; typeNodeIds: number[] }
  | { kind: typeof NODE_INTERSECTION; typeNodeIds: number[] }
  | { kind: typeof NODE_ARRAY; elementNodeId: number; readonly: boolean }
  | { kind: typeof NODE_TUPLE; readonly: boolean; elements: GraphTupleElementRecord[] }
  | { kind: typeof NODE_OBJECT; members: GraphObjectMemberRecord[] }
  | {
      kind: typeof NODE_FUNCTION;
      parameters: GraphFunctionParamRecord[];
      returnTypeNodeId: number;
      typeParameterNodeIds: number[];
    }
  | { kind: typeof NODE_REF; nameId: number; typeArgumentNodeIds: number[] }
  | {
      kind: typeof NODE_TYPE_PARAMETER;
      nameId: number;
      constraintNodeId: number;
      defaultNodeId: number;
    }
  | { kind: typeof NODE_KEY_OF; operandNodeId: number }
  | { kind: typeof NODE_TYPE_OF; pathIds: number[] }
  | { kind: typeof NODE_INDEXED_ACCESS; objectNodeId: number; indexNodeId: number }
  | {
      kind: typeof NODE_CONDITIONAL;
      checkNodeId: number;
      extendsNodeId: number;
      trueTypeNodeId: number;
      falseTypeNodeId: number;
    }
  | {
      kind: typeof NODE_MAPPED;
      parameterId: number;
      sourceNodeId: number;
      valueNodeId: number;
      optionalModifier: number;
      readonlyModifier: number;
      nameTypeNodeId: number;
    }
  | { kind: typeof NODE_TEMPLATE_LITERAL; quasiIds: number[]; expressionNodeIds: number[] }
  | { kind: typeof NODE_PARENTHESIZED; innerNodeId: number }
  | { kind: typeof NODE_UNKNOWN; rawId: number }
  | { kind: typeof NODE_INFER; nameId: number }
  | { kind: typeof NODE_REST; innerNodeId: number };

export class DecodedTypeGraph {
  readonly strings: readonly string[];
  readonly nodes: readonly GraphNodeRecord[];
  readonly descriptorMemo = new Map<number, TypeDescriptor>();
  readonly descriptorMemoByContext = new WeakMap<object, Map<number, TypeDescriptor>>();

  constructor(strings: string[], nodes: GraphNodeRecord[]) {
    this.strings = Object.freeze(strings.slice());
    this.nodes = Object.freeze(nodes.slice());
  }

  getString(id: number): string {
    if (id <= 0 || id > this.strings.length) {
      throw graphError(`component-meta graph missing string id ${id}`);
    }
    return this.strings[id - 1]!;
  }

  getStringMaybe(id: number): string | undefined {
    if (id === 0) {
      return undefined;
    }
    return this.getString(id);
  }

  getNode(id: number): GraphNodeRecord {
    if (id <= 0 || id > this.nodes.length) {
      throw graphError(`component-meta graph missing node id ${id}`);
    }
    return this.nodes[id - 1]!;
  }
}

export interface GraphTypeExprRef {
  readonly [GRAPH_TYPE_REF]: true;
  readonly graph: DecodedTypeGraph;
  readonly nodeId: number;
}

export function createGraphTypeExprRef(graph: DecodedTypeGraph, nodeId: number): GraphTypeExprRef {
  if (nodeId <= 0) {
    throw graphError(`component-meta graph missing node id ${nodeId}`);
  }
  graph.getNode(nodeId);
  return Object.freeze({
    [GRAPH_TYPE_REF]: true as const,
    graph,
    nodeId,
  });
}

export function isGraphTypeExprRef(value: unknown): value is GraphTypeExprRef {
  return Boolean(
    value &&
    typeof value === "object" &&
    GRAPH_TYPE_REF in (value as Record<PropertyKey, unknown>) &&
    (value as GraphTypeExprRef).nodeId > 0,
  );
}

function graphError(message: string): Error {
  return new Error(message);
}
