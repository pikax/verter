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

import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import {
  FrameworkSurfaceKind,
  GraphOperation,
  GraphProjectionMode,
  GraphReductionDemand,
  TYPEINFO_GRAPH_SCHEMA_VERSION,
  TypeInfoCapabilityHandshakeRequestSchema,
  TypeInfoCapabilityHandshakeResponseSchema,
  TypeInfoGraphRequestSchema,
} from "@verter/proto";
import { VerterHost, captureAvailability } from "@verter/native";
import { describe, expect, it } from "vitest";

import type { FunctionType, TypeDescriptor } from "@verter/type-ir";

import {
  decodeFrameworkSurfaceResponse,
  type FrameworkSurface,
  type FrameworkSurfaceError,
} from "../src/framework-surface.js";

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

/** The full facet demand: every known surface kind (schema 10 `requestedKinds`). */
const EVERY_KIND: FrameworkSurfaceKind[] = [
  FrameworkSurfaceKind.PROPS,
  FrameworkSurfaceKind.EMITS,
  FrameworkSurfaceKind.SLOTS,
  FrameworkSurfaceKind.OPTIONS,
  FrameworkSurfaceKind.EXPOSE,
  FrameworkSurfaceKind.MODEL,
];

/**
 * Discover the digest of the framework profile `host` advertises for
 * `family` through the real capability handshake binding — the only way a
 * client learns a profile reference (schema 11).
 */
function discoverProfile(host: VerterHost, family: string): Uint8Array {
  const request = create(TypeInfoCapabilityHandshakeRequestSchema, {
    clientVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
  });
  const handshake = fromBinary(
    TypeInfoCapabilityHandshakeResponseSchema,
    new Uint8Array(
      host.typeInfoCapabilities(
        Buffer.from(toBinary(TypeInfoCapabilityHandshakeRequestSchema, request)),
      ),
    ),
  );
  const digest = handshake.frameworkProfiles.find((p) => p.family === family)?.profile?.digest;
  if (!digest) {
    throw new Error(`the host advertises no ${family} profile`);
  }
  return digest;
}

/** Encode the wire `TypeInfoGraphRequest` envelope naming a profile digest. */
function encodeRequest(canonicalId: string, digest: Uint8Array): Buffer {
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
          frameworkProfile: { digest },
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
        requestedKinds: EVERY_KIND,
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

    const vueProfile = discoverProfile(host, "vue");
    const { response, auditRecord } = host.resolveFrameworkSurfaceWithAudit(
      encodeRequest(canonicalId, vueProfile),
    );
    expect(response).toBeInstanceOf(Buffer);
    // Audit is enabled, so the record rides the result exactly when the
    // binding captures (`semantic-observe`); a default binding resolves the
    // same surface and carries no record (an absent field crosses NAPI as
    // `undefined`).
    if (captureAvailability() === "Available") {
      expect(auditRecord).toBeInstanceOf(Buffer);
    } else {
      expect(auditRecord ?? null).toBeNull();
    }

    const decoded = decodeFrameworkSurfaceResponse(new Uint8Array(response));
    expect("error" in decoded).toBe(false);
    const surface = decoded as FrameworkSurface;

    // The decoded selector echo keeps the admitted profile qualification.
    expect(surface.selector?.canonicalId).toBe(canonicalId);
    expect(Array.from(surface.selector?.frameworkProfile?.digest ?? [])).toEqual(
      Array.from(vueProfile),
    );

    // The response carries exactly one entry per demanded kind.
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

  it("returns the typed unknownFrameworkProfile error for an unadmitted profile", () => {
    const host = new VerterHost({ auditEnabled: false });
    const canonicalId = "/fixtures/Unknown.vue";
    host.upsert({
      canonicalId,
      inputId: canonicalId,
      source: Buffer.from(VUE_SFC, "utf-8"),
    });

    // A well-shaped 32-byte reference the host never minted.
    const forged = new Uint8Array(32).fill(0xab);
    const { response } = host.resolveFrameworkSurfaceWithAudit(encodeRequest(canonicalId, forged));

    const decoded = decodeFrameworkSurfaceResponse(new Uint8Array(response));
    expect("error" in decoded).toBe(true);
    const { error } = decoded as FrameworkSurfaceError;
    expect(error.case).toBe("unknownFrameworkProfile");
    if (error.case === "unknownFrameworkProfile") {
      expect(Array.from(error.value.profile?.digest ?? [])).toEqual(Array.from(forged));
    }
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
      encodeRequest(canonicalId, discoverProfile(host, "svelte")),
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
