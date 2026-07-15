/**
 * AUTO-GENERATED from verter_protocol::schema.
 *
 * DO NOT EDIT MANUALLY. These types are the canonical TypeScript
 * representation of verter_protocol's schema DTOs. Changes should be
 * made in the Rust schema definitions and regenerated.
 *
 * Source: crates/verter_protocol/src/schema/
 */

// ── Refs (from schema/refs.rs) ─────────────────────────────────

export interface FileRefDto {
  fileId: string;
}

export interface BindingRefDto {
  fileId: string;
  bindingKey: string;
  name: string;
  spanStart: number;
  spanEnd: number;
}

export interface ComponentRefDto {
  fileId: string;
  componentKey: string;
  exportName: string;
  spanStart: number;
  spanEnd: number;
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
  spanStart: number;
  spanEnd: number;
}

export interface EventDto {
  name: string;
  payloadType?: string | null;
  description?: string | null;
  spanStart: number;
  spanEnd: number;
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
  spanStart: number;
  spanEnd: number;
}

export interface ModelDto {
  name: string;
  typeText?: string | null;
  spanStart: number;
  spanEnd: number;
}

export interface ExposeDto {
  name: string;
  spanStart: number;
  spanEnd: number;
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
  spanStart: number;
  spanEnd: number;
}

export interface ProvenanceStepDto {
  kind: string;
  description: string;
  spanStart: number;
  spanEnd: number;
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
