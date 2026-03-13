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
 * A string represents a simple type; an object represents a compound type.
 */
export type PropertyMetaSchema =
  | string
  | {
      kind: "enum" | "object" | "array";
      type: string;
      schema?: PropertyMetaSchema[];
    };

/**
 * Component metadata in Volar-compatible shape.
 * The `_verter` field provides opt-in access to the full Verter native metadata.
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
  /** Full Verter native metadata (opt-in extension). */
  _verter?: ComponentMeta;
}

/** Options for the meta checker. */
export interface MetaCheckerOptions {
  /** Whether to compute schemas. `false` disables schema computation. */
  schema?: boolean | { ignore?: (type: string) => boolean };
  /** Printer options (unused in Verter, kept for Volar compat). */
  printer?: unknown;
}
