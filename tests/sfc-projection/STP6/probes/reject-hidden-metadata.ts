import type { Precision } from "@stp6/lib/unpublished-meta";

/** Precision that exists only as producer-side unpublished metadata. */
export const leaked: Precision = { msg: "no", unpublished: true };
export const stp6HoverTarget = leaked;
export const stp6DefinitionTarget = leaked;
