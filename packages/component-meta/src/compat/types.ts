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

/**
 * Recursive schema type for property metadata.
 * Matches Volar's discriminated union: enum/array/event use arrays, object uses Record.
 * Intersection schemas use `Record<string, PropertyMetaSchema>` as a known divergence
 * (vue-component-meta flattens intersections to merged properties via TS checker).
 */
export type PropertyMetaSchema =
  | string
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
  schema?: boolean | { ignore?: (type: string) => boolean };
  /** Printer options (unused in Verter, kept for Volar compat). */
  printer?: unknown;
  /** Force TypeScript usage (no-op in Verter — always uses TS). Kept for Volar compat. */
  forceUseTs?: boolean;
  /** Select the type expansion backend used for component metadata queries. */
  typeExpansionBackend?: "verter" | "tsserver" | "tsgo" | "auto";
}
