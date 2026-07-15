// @ai-generated - Synthetic mapped and template-literal key typeinfo fixture.

export type VNode = unknown;

export interface SlotSource {
  root?: { id: string };
  item?: { value: number };
  empty?: { reason: "none" | "filtered" };
}

export type PlainSlotMap = {
  [K in keyof SlotSource]-?: SlotSource[K];
};

export type RemappedSlotMap = {
  [K in keyof SlotSource as K extends string ? `slot:${K}` : never]-?: (
    payload: NonNullable<SlotSource[K]>,
  ) => VNode[];
};

export type StaticTemplateSlots = {
  "cell:name": (payload: { value: string; column: "name" }) => VNode[];
  "cell:count": (payload: { value: number; column: "count" }) => VNode[];
};

export type TemplateLiteralCellName = `cell:${"name"}`;
export type NameCellRenderer = StaticTemplateSlots[TemplateLiteralCellName];
export type RootRemappedSlot = RemappedSlotMap["slot:root"];
export type ItemRemappedSlot = RemappedSlotMap["slot:item"];

export type RecordTemplateSlots = Record<
  `slot:${"root" | "item"}`,
  (payload: { name: "root" | "item" }) => VNode[]
>;
export type RecordTemplateRootSlot = RecordTemplateSlots["slot:root"];
export type StaticTemplateSlotUnion = StaticTemplateSlots[`cell:${"name" | "count"}`];
