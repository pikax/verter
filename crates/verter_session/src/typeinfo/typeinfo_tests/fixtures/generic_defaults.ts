// @ai-generated - Synthetic constrained generic default typeinfo fixture.

export interface BaseItem {
  id: string;
  label: string;
}

export interface DefaultItem extends BaseItem {
  badge?: "new" | "old";
}

export interface CustomItem extends BaseItem {
  count: number;
}

export type GenericBox<TItem extends BaseItem = DefaultItem, TValue extends string = "default"> = {
  item: TItem;
  value: TValue;
  list: TItem[];
  describe?: (item: TItem, value: TValue) => string;
};

export type DefaultGenericBox = GenericBox;
export type CustomGenericBox = GenericBox<CustomItem, "custom">;
export type ConstrainedPair<TValue extends string = "left", TMirror extends TValue = TValue> = {
  value: TValue;
  mirror: TMirror;
};
export type DefaultConstrainedPair = ConstrainedPair;
