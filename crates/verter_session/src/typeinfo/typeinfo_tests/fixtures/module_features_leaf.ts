// @ai-generated - Synthetic leaf for `typeof import("./...")` typeinfo
// tests. Exports both a default value and named values so the consumer
// can probe `.default` and named-export shapes.

export const leafName = "leaf";
export interface LeafShape {
  id: string;
  count: number;
}
export function leafFactory(): LeafShape {
  return { id: leafName, count: 0 };
}

const leafDefault = { tag: "leaf-default" as const, count: 0 };
export default leafDefault;
