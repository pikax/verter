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

import type { FunctionType, TypeDescriptor } from "@verter/type-ir";

import { decodeFrameworkSurfaceResponse, type FrameworkSurface } from "../src/framework-surface.js";

function expectFunction(type: TypeDescriptor | undefined): FunctionType {
  expect(type?.kind).toBe("function");
  if (type?.kind !== "function") {
    throw new Error(`expected a function descriptor, got ${String(type?.kind)}`);
  }
  return type;
}

const VUE_SFC = `<script setup lang="ts">
interface Props { count: number; label?: string }
defineProps<Props>();
defineEmits<{ change: [next: number] }>();
defineSlots<{ default(props: { item: string }): unknown }>();
</script>
<template><div></div></template>
`;

/** Encode the wire `TypeInfoGraphRequest` envelope for a registered adapter id. */
function encodeRequest(canonicalId: string, frameworkAdapterId: string): Buffer {
  const request = create(TypeInfoGraphRequestSchema, {
    schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
    operation: GraphOperation.FRAMEWORK_SURFACES,
    payload: {
      case: "frameworkSurface",
      value: {
        selector: { canonicalId, exportName: "", hasExportName: false, frameworkAdapterId },
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
      encodeRequest(canonicalId, "vue"),
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
    const count = props.members.find((m) => m.name === "count");
    expect(count?.type).toEqual({ kind: "primitive", name: "number" });

    const emits = surface.kinds.get(FrameworkSurfaceKind.EMITS)!;
    expect(emits.isSupported).toBe(true);
    expect(emits.members.map((m) => m.name)).toContain("change");

    const slots = surface.kinds.get(FrameworkSurfaceKind.SLOTS)!;
    expect(slots.isSupported).toBe(true);
    expect(slots.members.map((m) => m.name)).toContain("default");

    // Vue slot-binding type: the slot member is a callable whose first
    // parameter is the bindings object. An opaque node cannot claim this.
    const defaultSlot = slots.members.find((m) => m.name === "default");
    const slotFn = expectFunction(defaultSlot?.type);
    expect(slotFn.parameters[0]?.type).toEqual({
      kind: "object",
      properties: [{ name: "item", type: { kind: "primitive", name: "string" }, optional: false }],
    });
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

/**
 * The Svelte framework-surface response's OWN contract.
 *
 * Structural framework metadata answers names / requiredness / runtime
 * defaults AND the public type-bearing member DTO: inline callbacks
 * publish parameter/return structure, aliases stay shallow refs, and an
 * opaque node cannot claim a complete signature. Named-symbol query
 * (`tests/resolve-symbol.spec.ts`) remains the ECRS4 control — its
 * success does not close this member-type acceptance.
 *
 * REGRESSION — fails if member types are dropped, if a callback
 * signature is recovered from the `on` name prefix / display text, if
 * the derived callback-event surface starts trusting the `on` name
 * prefix instead of the resolved callable arm, or if the requiredness /
 * default projection drifts.
 */

const SVELTE_CALLBACKS = `<script lang="ts">
type Handler = (id: string) => void;
interface Props {
  onMove: (x: number, y: number) => void;
  onToggle?: (on?: boolean) => void;
  onSelect: Handler;
  onlabel: string;
  label: string;
}
let { onMove, onToggle, onSelect, onlabel, label = "untitled" }: Props = $props();
</script>
<div>{label}</div>
`;

describe("resolveFrameworkSurfaceWithAudit (Svelte callback props)", () => {
  it("answers names / requiredness / defaults and type-bearing callback members", () => {
    const host = new VerterHost({ auditEnabled: false });
    const canonicalId = "/fixtures/Callbacks.svelte";
    host.upsert({
      canonicalId,
      inputId: canonicalId,
      source: Buffer.from(SVELTE_CALLBACKS, "utf-8"),
    });

    const { response } = host.resolveFrameworkSurfaceWithAudit(
      encodeRequest(canonicalId, "svelte"),
    );
    const decoded = decodeFrameworkSurfaceResponse(new Uint8Array(response));
    expect("error" in decoded).toBe(false);
    const surface = decoded as FrameworkSurface;

    const props = surface.kinds.get(FrameworkSurfaceKind.PROPS)!;
    expect(props.isSupported).toBe(true);
    expect(props.members.map((m) => m.name)).toEqual([
      "onMove",
      "onToggle",
      "onSelect",
      "onlabel",
      "label",
    ]);
    // Requiredness is the DESTRUCTURING-aware Svelte answer: the `?`-optional
    // member and the member with a destructuring default are both optional;
    // the rest are required.
    expect(props.members.map((m) => m.required)).toEqual([true, false, true, true, false]);
    // Defaults are runtime expression source text, present only where authored.
    expect(props.members.map((m) => m.default)).toEqual([
      undefined,
      undefined,
      undefined,
      undefined,
      '"untitled"',
    ]);

    // The derived callback-event surface keys off the RESOLVED callable arm,
    // not the `on` name prefix: `onlabel: string` is a prop but never an
    // event, while the aliased `onSelect: Handler` resolves to a callable and
    // is. A response that trusted the name alone would list `label` here.
    const emits = surface.kinds.get(FrameworkSurfaceKind.EMITS)!;
    expect(emits.isSupported).toBe(true);
    expect([...emits.members.map((m) => m.name)].sort()).toEqual(["Move", "Select", "Toggle"]);

    const byName = Object.fromEntries(props.members.map((m) => [m.name, m]));
    const onMove = expectFunction(byName.onMove?.type);
    expect(onMove.parameters.map((p) => p.name)).toEqual(["x", "y"]);
    expect(onMove.parameters.map((p) => p.type)).toEqual([
      { kind: "primitive", name: "number" },
      { kind: "primitive", name: "number" },
    ]);
    expect(onMove.returnType).toEqual({ kind: "primitive", name: "void" });

    const onToggle = expectFunction(byName.onToggle?.type);
    expect(onToggle.parameters).toHaveLength(1);
    expect(onToggle.parameters[0]?.name).toBe("on");
    expect(onToggle.parameters[0]?.optional).toBe(true);
    expect(onToggle.parameters[0]?.type).toEqual({ kind: "primitive", name: "boolean" });

    // Shallow: the alias is a ref, not an inlined complete signature.
    expect(byName.onSelect?.type).toMatchObject({
      kind: "ref",
      name: "Handler",
    });
    // Name prefix is not callable: `onlabel` stays a primitive.
    expect(byName.onlabel?.type).toEqual({ kind: "primitive", name: "string" });
    expect(byName.label?.type).toEqual({ kind: "primitive", name: "string" });

    host.close();
  });
});
