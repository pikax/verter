/**
 * AUTO-GENERATED from verter_protocol::schema.
 *
 * DO NOT EDIT MANUALLY. These types are the canonical TypeScript
 * representation of verter_protocol's schema DTOs. Changes should be
 * made in the Rust schema definitions and regenerated.
 *
 * Source: crates/verter_protocol/src/schema/
 */

// ── Coordinates (from types.rs) ────────────────────────────────

/**
 * Versioned tag naming the unit a span is measured in (0-based, half-open,
 * counted from the start of the file the owning record names). A consumer MUST
 * reject a tag it does not know.
 */
export type SpanEncodingTag = "utf8.v1" | "utf16.v1";

/** A range that names its own encoding. */
export interface TaggedSpan {
  start: number;
  end: number;
  encoding: SpanEncodingTag;
}

// ── Refs (from schema/refs.rs) ─────────────────────────────────

export interface FileRefDto {
  fileId: string;
}

export interface BindingRefDto {
  fileId: string;
  bindingKey: string;
  name: string;
  span: TaggedSpan;
}

export interface ComponentRefDto {
  fileId: string;
  componentKey: string;
  exportName: string;
  span: TaggedSpan;
}

// ── Query (from schema/query.rs) ───────────────────────────────

export type CompletenessDto = "complete" | "partial" | "unavailable";

export interface RevisionMarkerDto {
  workspaceRevision: number;
  parserRevision: number;
  compilerRevision: number;
  providerRevision: number;
}

export interface QueryResultDto<T> {
  value: T;
  revision: RevisionMarkerDto;
  completeness: CompletenessDto;
  missingInputs?: string[];
  staleRef?: boolean;
}

// ── Component (from schema/component.rs) ───────────────────────

export interface PropDto {
  name: string;
  isOptional?: boolean;
  typeText?: string | null;
  defaultValue?: string | null;
  description?: string | null;
  span: TaggedSpan;
}

export interface EventDto {
  name: string;
  payloadType?: string | null;
  description?: string | null;
  span: TaggedSpan;
}

export interface SlotBindingDto {
  name: string;
  typeText?: string | null;
}

export interface SlotDto {
  name: string;
  isRequired?: boolean;
  bindings?: SlotBindingDto[];
  description?: string | null;
  span: TaggedSpan;
}

export interface ModelDto {
  name: string;
  typeText?: string | null;
  span: TaggedSpan;
}

export interface ExposeDto {
  name: string;
  span: TaggedSpan;
}

export interface ComponentSurfaceDto {
  props: PropDto[];
  events: EventDto[];
  slots: SlotDto[];
  models: ModelDto[];
  expose?: ExposeDto[];
  completeness?: string | null;
  inheritAttrsDisabled?: boolean;
}

export interface BoundaryIssueDto {
  kind: string;
  componentName: string;
  memberName: string;
  span: TaggedSpan;
}

export interface ProvenanceStepDto {
  kind: string;
  description: string;
  span: TaggedSpan;
}

export interface ReactivityDto {
  status: string;
  source?: string | null;
  trace?: ProvenanceStepDto[];
}

// ── Runtime Schema ─────────────────────────────────────────────

export interface RuntimePropSchema {
  name: string;
  required: boolean;
  typeText?: string | null;
  defaultValue?: string | null;
}

export interface RuntimeModelSchema {
  name: string;
  typeText?: string | null;
}

export interface RuntimeEventSchema {
  name: string;
  payloadType?: string | null;
}

export interface RuntimeSlotSchema {
  name: string;
  required: boolean;
}

export interface ComponentRuntimeSchema {
  props: RuntimePropSchema[];
  models: RuntimeModelSchema[];
  events: RuntimeEventSchema[];
  slots: RuntimeSlotSchema[];
}
