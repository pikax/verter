/**
 * Volar-compatible type definitions for vue-component-meta drop-in replacement.
 *
 * These types mirror Volar's `vue-component-meta` exactly so consumers
 * (e.g. nuxt-component-meta, nuxt-ui docs) can swap with zero code changes.
 */

import type { ComponentMeta } from "../types.js";

/** JSDoc tag — matches Volar's Tag interface. */
export interface Tag {
  name: string;
  text?: string;
}

/**
 * Property metadata — used for props, events, slots, and exposed members.
 * Matches Volar's `PropertyMeta` shape exactly.
 */
export interface PropertyMeta {
  name: string;
  description: string;
  type: string;
  default?: string;
  required: boolean;
  global?: boolean;
  tags: Tag[];
  schema: PropertyMetaSchema;
}

export interface CompatSchemaOptions {
  ignore?: (type: string) => boolean;
  /**
   * Expand `boolean` schema nodes to `true | false` enum members for parity-focused
   * callers without changing native descriptors or compat display text.
   */
  literalBooleanSchema?: boolean;
}

/**
 * Recursive schema type for property metadata.
 * Matches Volar's discriminated union: enum/array/event use arrays, object uses Record.
 * Intersection schemas use `Record<string, PropertyMetaSchema>` as a known divergence
 * (vue-component-meta flattens intersections to merged properties via TS checker).
 */
export type PropertyMetaSchema =
  | string
  | PropertyMetaSchema[]
  | { kind: "enum"; type: string; schema?: PropertyMetaSchema[] }
  | { kind: "array"; type: string; schema?: PropertyMetaSchema[] }
  | { kind: "event"; type: string; schema?: PropertyMetaSchema[] }
  | {
      kind: "object";
      type: string;
      schema?: Record<string, PropertyMeta> | Record<string, PropertyMetaSchema>;
    };

/**
 * Component metadata in Volar-compatible shape.
 * The `_verter` field provides opt-in access to the full Verter mapped metadata.
 */
export interface VolarComponentMeta {
  /** Component type (0 = component). */
  type: number;
  /** Props as Volar PropertyMeta. */
  props: PropertyMeta[];
  /** Events as Volar PropertyMeta. */
  events: PropertyMeta[];
  /** Slots as Volar PropertyMeta. */
  slots: PropertyMeta[];
  /** Exposed members as Volar PropertyMeta. */
  exposed: PropertyMeta[];
  /** Full Verter mapped metadata (opt-in extension). */
  _verter?: ComponentMeta;
}

/** Options for the meta checker. */
export interface MetaCheckerOptions {
  /** Whether to compute schemas. `false` disables schema computation. */
  schema?: boolean | CompatSchemaOptions;
  /** Printer options (unused in Verter, kept for Volar compat). */
  printer?: unknown;
  /** Force TypeScript usage (no-op in Verter — always uses TS). Kept for Volar compat. */
  forceUseTs?: boolean;
  /**
   * Runtime ownership mode.
   * `shared` reuses the process-global pooled runtime.
   * `dedicated` creates an isolated runtime for one checker/session instance.
   */
  runtimeMode?: "shared" | "dedicated";
  /**
   * Logging/audit settings. When `audit` is true, the native runtime captures
   * per-request timing, memory, and solver cost data as structured
   * `RustAuditRecord` artifacts. Default: false (zero overhead).
   */
  logging?: {
    audit?: boolean;
  };
}
