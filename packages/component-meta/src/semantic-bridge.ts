/**
 * Bridge to verter_semantic runtime schema types.
 *
 * Re-exports the generated protocol types from @verter/language-shared
 * so component-meta consumers can access the shared semantic pipeline's
 * type definitions without importing from multiple packages.
 */

export type {
  ComponentRuntimeSchema,
  RuntimePropSchema,
  RuntimeModelSchema,
  RuntimeEventSchema,
  RuntimeSlotSchema,
  ComponentSurfaceDto,
  PropDto,
  EventDto,
  SlotDto,
  ModelDto,
  ExposeDto,
  BoundaryIssueDto,
  ReactivityDto,
  QueryResultDto,
  RevisionMarkerDto,
  CompletenessDto,
} from "@verter/language-shared";
