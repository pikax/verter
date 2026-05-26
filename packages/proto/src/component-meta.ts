import type { Message, MessageInitShape } from "@bufbuild/protobuf";

import {
  ComponentMetaPayloadSchema,
  OriginGraphSchema,
  type ComponentMetaPayload as RawComponentMetaPayload,
  type TypeGraph as RawTypeGraph,
  type TypeNode as RawTypeNode,
  type OriginGraph as RawOriginGraph,
  type OriginNode as RawOriginNode,
  type OriginEdge as RawOriginEdge,
} from "./gen/verter/v1/component_meta_pb.js";

export { ComponentMetaPayloadSchema, OriginGraphSchema };

export type ProtoRecord<TypeName extends string = string> = Message<TypeName> & Record<string, any>;

export type ProtoTypeNode = ProtoRecord<"verter.v1.TypeNode"> & RawTypeNode;
export type ProtoTypeGraph = ProtoRecord<"verter.v1.TypeGraph"> & RawTypeGraph;
export type ComponentMetaPayload = ProtoRecord<"verter.v1.ComponentMetaPayload"> &
  RawComponentMetaPayload;

export type ComponentMetaPayloadInit = MessageInitShape<typeof ComponentMetaPayloadSchema>;
export type OriginGraphInit = MessageInitShape<typeof OriginGraphSchema>;

export type ProtoOriginGraph = ProtoRecord<"verter.v1.OriginGraph"> & RawOriginGraph;
export type ProtoOriginNode = ProtoRecord<"verter.v1.OriginNode"> & RawOriginNode;
export type ProtoOriginEdge = ProtoRecord<"verter.v1.OriginEdge"> & RawOriginEdge;

const SCHEMA_VERSION = 2;
const PRIMITIVE_STRING = 1;
const PRIMITIVE_UNDEFINED = 11;
const OBJECT_MEMBER_PROPERTY = 1;
const ACCEPTED_SURFACE_COMPLETENESS_EXACT = 1;
const ROOT_REACHABILITY_NO_FALLTHROUGH = 1;
const FALLTHROUGH_SURFACE_NONE = 1;
const NO_FALLTHROUGH_REASON_NO_TEMPLATE = 5;

type TypeNodeInit = NonNullable<
  NonNullable<ComponentMetaPayloadInit["typeGraph"]>["nodes"]
>[number];

function typeNode(
  caseName: "primitive" | "ref" | "union" | "object" | "recursiveRef",
  value: Record<string, unknown>,
): TypeNodeInit {
  return {
    kind: {
      case: caseName as TypeNodeInit["kind"] extends { case: infer C } ? C : never,
      value: value as never,
    },
  };
}

export function createTestComponentMetaPayload(): ComponentMetaPayloadInit {
  return {
    schemaVersion: SCHEMA_VERSION,
    typeGraph: {
      strings: ["/src/Tree.vue", "TreeNode", "root", "default", "VNode[]", "label", "next"],
      nodes: [
        typeNode("primitive", { primitive: PRIMITIVE_STRING }),
        typeNode("primitive", { primitive: PRIMITIVE_UNDEFINED }),
        typeNode("recursiveRef", {
          nameId: 2,
          typeArgumentNodeIds: [1],
          conditionalContext: [
            {
              branch: 1,
              decided: true,
              checkNodeId: 1,
              extendsNodeId: 1,
            },
          ],
        }),
        typeNode("union", { typeNodeIds: [3, 2] }),
        typeNode("object", {
          members: [
            {
              kind: OBJECT_MEMBER_PROPERTY,
              nameId: 6,
              typeNodeId: 1,
              optional: false,
              readonly: false,
              keyNameId: 0,
              keyTypeNodeId: 0,
              valueTypeNodeId: 0,
              functionNodeId: 0,
            },
            {
              kind: OBJECT_MEMBER_PROPERTY,
              nameId: 7,
              typeNodeId: 4,
              optional: true,
              readonly: false,
              keyNameId: 0,
              keyTypeNodeId: 0,
              valueTypeNodeId: 0,
              functionNodeId: 0,
            },
          ],
        }),
      ],
    },
    typeRegistry: [
      {
        nameId: 2,
        typeNodeId: 5,
        rawTypeId: 2,
      },
    ],
    body: {
      filePathId: 1,
      optionsApi: false,
      props: [
        {
          nameId: 3,
          typeNodeId: 3,
          rawTypeId: 2,
          required: true,
          hasDefault: false,
          tags: [],
        },
      ],
      events: [],
      slots: [
        {
          nameId: 4,
          isScoped: true,
          bindings: [
            {
              nameId: 3,
              typeNodeId: 3,
              rawTypeId: 2,
            },
          ],
          isRequired: false,
          returnTypeId: 5,
          tags: [],
        },
      ],
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
        kind: FALLTHROUGH_SURFACE_NONE,
        reason: NO_FALLTHROUGH_REASON_NO_TEMPLATE,
        branches: [],
      },
    },
  };
}
