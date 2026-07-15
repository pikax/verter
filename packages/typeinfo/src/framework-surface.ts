/**
 * Typed decode of the framework-surface wire payload.
 *
 * The native binding `VerterHost.resolveFrameworkSurfaceWithAudit`
 * returns a protobuf-encoded `TypeInfoGraphResponse` (the
 * `framework_surface` arm on success, the `error` arm on a typed
 * rejection). This module decodes those bytes into an ergonomic
 * TypeScript surface:
 *
 * - {@link FrameworkSurface} — `framework` tag + a per-kind map carrying
 *   each kind's support status and resolved members (member names are
 *   resolved through the graph string table here, so consumers never
 *   touch the interned id space).
 * - {@link FrameworkSurfaceError} — the typed wire error arm.
 *
 * **Status semantics.** Per-kind status is surfaced VERBATIM from
 * the wire `FrameworkSurfaceKindStatus`: a SUPPORTED kind with zero
 * members is supported-empty (a real-but-empty surface), DISTINCT from
 * an UNSUPPORTED kind (which also carries zero members). The decoded
 * `isSupported` / `isUnsupported` flags preserve that distinction so a
 * consumer never has to infer support from member-count emptiness.
 *
 * Kept free of any `@verter/native` import so the decode can be
 * unit-tested without loading the native host binary — the input is raw
 * wire bytes, exactly as the binding emits them.
 */

import { fromBinary } from "@bufbuild/protobuf";
import {
  FrameworkSurfaceDeclarationKind,
  type FrameworkSurfaceKind,
  FrameworkSurfaceKindSupport,
  type FrameworkSurfaceMember as WireFrameworkSurfaceMember,
  type FrameworkSurfaceMemberOrigin as WireFrameworkSurfaceMemberOrigin,
  FrameworkSurfaceOriginHopKind,
  type FrameworkSurfacePayload,
  type FrameworkTag,
  TypeInfoGraphResponseSchema,
  type TypeInfoRequestError,
} from "@verter/proto";

/**
 * One hop in a framework-surface member's declaration ORIGIN chain
 * (schema 4). `kind` selects which fields are populated.
 *
 * Every string field is PRESENCE-AWARE: it is `undefined` when the wire
 * hop did not set it (the `optional` has-bit is unset), distinct from a
 * field whose interned string happens to live at table index 0. A LOCAL
 * hop carries no string fields; an IMPORT hop carries `from` +
 * `importedName` (and `specifier` only when recorded); a REEXPORT hop
 * carries `from` / `to` / `exportedName` / `originalName`; an ALIAS hop
 * carries `aliasName`. An absent field is NEVER resolved through the
 * string table.
 */
export interface FrameworkSurfaceOriginHop {
  /** The hop kind (LOCAL / IMPORT / REEXPORT / ALIAS). */
  readonly kind: FrameworkSurfaceOriginHopKind;
  /** Import / Reexport: the source module canonical (`undefined` when N/A). */
  readonly from?: string;
  /** Import: the raw import specifier, when recorded (`undefined` when absent). */
  readonly specifier?: string;
  /** Import: the imported name in the source module (`undefined` when N/A). */
  readonly importedName?: string;
  /** Reexport: the module the symbol re-exports to (`undefined` when N/A). */
  readonly to?: string;
  /** Reexport: the name the symbol is re-exported under (`undefined` when N/A). */
  readonly exportedName?: string;
  /** Reexport: the original name before the rename (`undefined` when N/A). */
  readonly originalName?: string;
  /** Alias: the alias target name (`undefined` when N/A). */
  readonly aliasName?: string;
}

/** A framework-surface member's resolved declaration (schema 4). */
export interface FrameworkSurfaceMemberDeclaration {
  /** The requested name, resolved through the graph string table. */
  readonly requestedName: string;
  /** The resolved name, resolved through the graph string table. */
  readonly resolvedName: string;
  /** The declaring file canonical, resolved through the string table. */
  readonly canonicalSource: string;
  /** Declaration span start (byte offset in `canonicalSource`). */
  readonly spanStart: number;
  /** Declaration span end (byte offset in `canonicalSource`). */
  readonly spanEnd: number;
  /** The declaration kind. */
  readonly kind: FrameworkSurfaceDeclarationKind;
}

/**
 * A framework-surface member's declaration ORIGIN (schema 4): the
 * resolver-known per-member declaration plus the ordered hop chain to
 * it. Populated only from routes the shared resolver traversed.
 */
export interface FrameworkSurfaceMemberOrigin {
  /**
   * The per-member declaration, when one was resolved. `undefined` for an
   * inline/local member with a declaration file but no separately-named
   * declaration.
   */
  readonly declaration?: FrameworkSurfaceMemberDeclaration;
  /** The ordered hop chain from the requesting file to the declaration. */
  readonly chain: readonly FrameworkSurfaceOriginHop[];
}

/** One resolved member of a framework surface (props/emits/slots/…). */
export interface FrameworkSurfaceMember {
  /** The member name, resolved through the graph string table. */
  readonly name: string;
  /** Whether the member is required (non-optional). */
  readonly required: boolean;
  /** Whether the member is readonly. */
  readonly readonly: boolean;
  /**
   * The member's runtime DEFAULT value source text (schema 4), resolved
   * through the graph string table. `undefined` when the member has no
   * default. Defaults are runtime expressions, not types.
   */
  readonly default?: string;
  /**
   * The member's resolver-known declaration ORIGIN (schema 4).
   * `undefined` when no origin was resolver-known (a synthetic /
   * multi-origin member, or an adapter that does not derive origins).
   */
  readonly origin?: FrameworkSurfaceMemberOrigin;
}

/** A single framework-surface kind's resolved status and members. */
export interface FrameworkSurfaceKindResult {
  /** The wire support status (SUPPORTED / UNSUPPORTED / PARTIAL / …). */
  readonly support: FrameworkSurfaceKindSupport;
  /**
   * `true` when the kind is SUPPORTED. A SUPPORTED kind with zero
   * {@link members} is supported-empty — a real surface that is empty,
   * NOT unsupport.
   */
  readonly isSupported: boolean;
  /** `true` when the kind is UNSUPPORTED. */
  readonly isUnsupported: boolean;
  /** `true` when the kind is PARTIAL (a usable subset). */
  readonly isPartial: boolean;
  /** The resolved members (empty for supported-empty / unsupported). */
  readonly members: readonly FrameworkSurfaceMember[];
  /** Per-kind diagnostics, resolved through the graph string table. */
  readonly diagnostics: readonly string[];
}

/** The decoded `framework_surface` response arm. */
export interface FrameworkSurface {
  /** The wire framework tag (e.g. `FrameworkTag.VUE`). */
  readonly framework: FrameworkTag;
  /**
   * Per-kind resolved surfaces. A v3 payload carries exactly one entry
   * per known {@link FrameworkSurfaceKind}.
   */
  readonly kinds: ReadonlyMap<FrameworkSurfaceKind, FrameworkSurfaceKindResult>;
}

/** The decoded `error` response arm — the TYPED wire error variant. */
export interface FrameworkSurfaceError {
  /**
   * The typed error discriminant. `error.case` is the wire
   * `TypeInfoRequestError` oneof variant name (e.g. `"malformedPayload"`);
   * `error.value` is its typed payload (e.g. `{ detail: string }`). This is
   * the structural error, never a stringified display.
   */
  readonly error: TypeInfoRequestError["kind"];
}

/**
 * Decode the protobuf-encoded `TypeInfoGraphResponse` bytes returned by
 * `VerterHost.resolveFrameworkSurfaceWithAudit` into a
 * {@link FrameworkSurface} (the `framework_surface` arm) or a
 * {@link FrameworkSurfaceError} (the `error` arm).
 *
 * The native binding always produces a typed response (validation-first
 * executor), so this never throws on a well-formed buffer; a malformed
 * buffer surfaces as a `fromBinary` decode error.
 */
export function decodeFrameworkSurfaceResponse(
  bytes: Uint8Array,
): FrameworkSurface | FrameworkSurfaceError {
  const response = fromBinary(TypeInfoGraphResponseSchema, bytes);
  const kind = response.kind;

  if (kind.case === "frameworkSurface") {
    return decodePayload(kind.value);
  }
  if (kind.case === "error") {
    // Surface the TYPED error oneof (`{ case, value }`) verbatim — never a
    // stringified display. The framework-surface operation never produces
    // the `graph` arm, so this is the only error path.
    return { error: kind.value.kind };
  }
  // The `graph` arm is never produced for a framework-surface request, and
  // an empty `kind` is malformed — both surface as a typed-unspecified
  // error variant rather than a fabricated string.
  return { error: { case: undefined } as TypeInfoRequestError["kind"] };
}

function decodePayload(payload: FrameworkSurfacePayload): FrameworkSurface {
  const strings = payload.graph?.strings?.entries ?? [];

  const kinds = new Map<FrameworkSurfaceKind, FrameworkSurfaceKindResult>();
  for (const entry of payload.surfaces) {
    const support = entry.status?.support ?? FrameworkSurfaceKindSupport.UNSPECIFIED;
    const members: FrameworkSurfaceMember[] = entry.members.map((m) => decodeMember(strings, m));
    const diagnostics: string[] =
      entry.status?.diagnostics.map((d) => resolveString(strings, d.messageNameId)) ?? [];

    kinds.set(entry.kind, {
      support,
      isSupported: support === FrameworkSurfaceKindSupport.SUPPORTED,
      isUnsupported: support === FrameworkSurfaceKindSupport.UNSUPPORTED,
      isPartial: support === FrameworkSurfaceKindSupport.PARTIAL,
      members,
      diagnostics,
    });
  }

  return { framework: payload.framework, kinds };
}

/**
 * Decode one wire member into the public {@link FrameworkSurfaceMember},
 * resolving the name + (schema 4) the runtime default source text and the
 * declaration origin through the graph string table.
 *
 * `default` is presence-aware: the wire `default_value_id` is `optional`,
 * so an absent default (no field) decodes to `undefined`, distinct from a
 * default whose interned string id is 0. `origin` is `undefined` unless the
 * member carried a resolver-known origin.
 */
function decodeMember(
  strings: readonly string[],
  m: WireFrameworkSurfaceMember,
): FrameworkSurfaceMember {
  return {
    name: resolveString(strings, m.nameId),
    required: m.required,
    readonly: m.readonly,
    default: m.defaultValueId === undefined ? undefined : resolveString(strings, m.defaultValueId),
    origin: m.origin === undefined ? undefined : decodeMemberOrigin(strings, m.origin),
  };
}

/** Decode a wire member origin into the public shape (string ids resolved). */
function decodeMemberOrigin(
  strings: readonly string[],
  origin: WireFrameworkSurfaceMemberOrigin,
): FrameworkSurfaceMemberOrigin {
  const declaration =
    origin.declaration === undefined
      ? undefined
      : {
          requestedName: resolveString(strings, origin.declaration.requestedNameId),
          resolvedName: resolveString(strings, origin.declaration.resolvedNameId),
          canonicalSource: resolveString(strings, origin.declaration.canonicalSourceId),
          spanStart: origin.declaration.spanStart,
          spanEnd: origin.declaration.spanEnd,
          kind: origin.declaration.kind,
        };
  const chain: FrameworkSurfaceOriginHop[] = origin.chain.map((hop) => ({
    kind: hop.kind,
    // PRESENCE-AWARE: each hop string id is `optional` on the wire, so an
    // unset field is `undefined` here — NEVER resolved through the string
    // table (the graph table is zero-based, so id 0 is a real entry, not an
    // absent sentinel). Only a present id resolves to its interned string.
    from: resolveOptionalString(strings, hop.fromId),
    specifier: resolveOptionalString(strings, hop.specifierId),
    importedName: resolveOptionalString(strings, hop.importedNameId),
    to: resolveOptionalString(strings, hop.toId),
    exportedName: resolveOptionalString(strings, hop.exportedNameId),
    originalName: resolveOptionalString(strings, hop.originalNameId),
    aliasName: resolveOptionalString(strings, hop.aliasNameId),
  }));
  return { declaration, chain };
}

/**
 * Resolve an interned string-table index to its string, or the empty
 * string when out of range — the decode never throws on a malformed
 * index (a structurally-broken payload).
 */
function resolveString(strings: readonly string[], id: number): string {
  return strings[id] ?? "";
}

/**
 * Resolve a PRESENCE-AWARE (wire-`optional`) string id: `undefined` when
 * the field was not set on the wire (genuinely absent), otherwise the
 * interned string. An absent field is never resolved through the string
 * table — this is the guard against the zero-based-table-vs-0-sentinel
 * collision (the graph table's entry 0 is a real interned string, so a
 * plain id-0 absent sentinel would fabricate it).
 */
function resolveOptionalString(
  strings: readonly string[],
  id: number | undefined,
): string | undefined {
  return id === undefined ? undefined : resolveString(strings, id);
}
