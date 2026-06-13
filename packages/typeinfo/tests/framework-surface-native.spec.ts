/**
 * End-to-end framework-surface binding proof (the B5 keystone).
 *
 * A real `.vue` SFC is upserted into a live `VerterHost`; the request
 * envelope is encoded with `@verter/proto`, passed to the native
 * `resolveFrameworkSurfaceWithAudit` binding, and the returned wire
 * `TypeInfoGraphResponse` bytes are decoded by `decodeFrameworkSurfaceResponse`.
 *
 * The decoded props/emits/slots surfaces must carry the macro members the
 * SFC declares — the proof that the binding round-trips the host's
 * framework-surface executor output faithfully across the FFI boundary.
 *
 * REGRESSION — fails if the native method is missing, the wire encode /
 * decode drifts, or the executor stops surfacing the macro members.
 */

import { create, toBinary } from "@bufbuild/protobuf";
import {
  FrameworkSurfaceKind,
  GraphOperation,
  GraphProjectionMode,
  GraphReductionDemand,
  TYPEINFO_GRAPH_SCHEMA_VERSION,
  TypeInfoGraphRequestSchema,
} from "@verter/proto";
import { VerterHost } from "@verter/native";
import { describe, expect, it } from "vitest";

import { decodeFrameworkSurfaceResponse, type FrameworkSurface } from "../src/framework-surface.js";

const VUE_SFC = `<script setup lang="ts">
interface Props { count: number; label?: string }
defineProps<Props>();
defineEmits<{ change: [next: number] }>();
defineSlots<{ default(props: { item: string }): unknown }>();
</script>
<template><div></div></template>
`;

/** Encode the wire `TypeInfoGraphRequest` envelope for a Vue component. */
function encodeRequest(canonicalId: string): Buffer {
  const request = create(TypeInfoGraphRequestSchema, {
    schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
    operation: GraphOperation.FRAMEWORK_SURFACES,
    payload: {
      case: "frameworkSurface",
      value: {
        selector: {
          canonicalId,
          exportName: "",
          hasExportName: false,
          frameworkAdapterId: "vue",
        },
        context: {
          mode: GraphProjectionMode.NAVIGATE,
          demand: GraphReductionDemand.PUBLISHED,
        },
        closure: { kind: { case: "oneLevel", value: {} } },
        displayPolicy: {
          qualification: 1,
          branding: 1,
          budgets: { maxStringLength: 4096, maxDepth: 16 },
        },
        includeProvenance: false,
        includeDiagnostics: true,
        includeProjection: [],
        schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      },
    },
  });
  return Buffer.from(toBinary(TypeInfoGraphRequestSchema, request));
}

describe("resolveFrameworkSurfaceWithAudit (native binding)", () => {
  it("round-trips a Vue SFC's props/emits/slots through the wire", () => {
    const host = new VerterHost({ auditEnabled: true });
    const canonicalId = "/fixtures/Parity.vue";
    host.upsert({
      canonicalId,
      inputId: canonicalId,
      source: Buffer.from(VUE_SFC, "utf-8"),
    });

    const { response, auditRecord } = host.resolveFrameworkSurfaceWithAudit(
      encodeRequest(canonicalId),
    );
    expect(response).toBeInstanceOf(Buffer);
    // Audit is enabled, so the record rides the result.
    expect(auditRecord).not.toBeNull();

    const decoded = decodeFrameworkSurfaceResponse(new Uint8Array(response));
    expect("error" in decoded).toBe(false);
    const surface = decoded as FrameworkSurface;

    // A v3 payload carries exactly one entry per known kind.
    expect(surface.kinds.size).toBe(6);

    const props = surface.kinds.get(FrameworkSurfaceKind.PROPS)!;
    expect(props.isSupported).toBe(true);
    const propNames = props.members.map((m) => m.name).sort();
    expect(propNames).toEqual(["count", "label"]);

    const emits = surface.kinds.get(FrameworkSurfaceKind.EMITS)!;
    expect(emits.isSupported).toBe(true);
    expect(emits.members.map((m) => m.name)).toContain("change");

    const slots = surface.kinds.get(FrameworkSurfaceKind.SLOTS)!;
    expect(slots.isSupported).toBe(true);
    expect(slots.members.map((m) => m.name)).toContain("default");
  });

  it("returns the typed error arm for an unknown adapter id", () => {
    const host = new VerterHost({ auditEnabled: false });
    const canonicalId = "/fixtures/Unknown.vue";
    host.upsert({
      canonicalId,
      inputId: canonicalId,
      source: Buffer.from(VUE_SFC, "utf-8"),
    });

    // Hand-encode an envelope naming a non-existent adapter id.
    const request = create(TypeInfoGraphRequestSchema, {
      schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      operation: GraphOperation.FRAMEWORK_SURFACES,
      payload: {
        case: "frameworkSurface",
        value: {
          selector: {
            canonicalId,
            exportName: "",
            hasExportName: false,
            frameworkAdapterId: "not-a-real-framework",
          },
          context: {
            mode: GraphProjectionMode.NAVIGATE,
            demand: GraphReductionDemand.PUBLISHED,
          },
          closure: { kind: { case: "oneLevel", value: {} } },
          displayPolicy: {
            qualification: 1,
            branding: 1,
            budgets: { maxStringLength: 4096, maxDepth: 16 },
          },
          includeProvenance: false,
          includeDiagnostics: true,
          includeProjection: [],
          schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
        },
      },
    });
    const { response } = host.resolveFrameworkSurfaceWithAudit(
      Buffer.from(toBinary(TypeInfoGraphRequestSchema, request)),
    );

    const decoded = decodeFrameworkSurfaceResponse(new Uint8Array(response));
    expect("error" in decoded).toBe(true);
  });
});
