// @ai-generated - Synthetic component-shaped typeinfo fixture.

export type ColorToken = "neutral" | "primary" | "danger";
export type SizeToken = "xs" | "sm" | "md" | "lg";

export type PrimitiveSurface = {
  label: string;
  disabled?: boolean;
  count: number;
  variant?: "solid" | "ghost";
};

export type GenericBox<TValue> = {
  value: TValue;
  list: TValue[];
  maybe?: TValue | null;
};

export interface KeyedItem {
  id: string;
  label: string;
  disabled?: boolean;
  data?: {
    score: number;
    tags: string[];
  };
}

export interface ExternalSettings<TItem extends KeyedItem = KeyedItem> {
  size?: SizeToken;
  tone?: ColorToken;
  items?: TItem[];
  debugOnly?: {
    trace: boolean;
    sink: (event: string) => void;
  };
  lazy?: () => Promise<TItem[]>;
}

export type SlotNames = "root" | "label" | "item" | "empty";
export type StyleConfig<TSlot extends string = SlotNames> = Partial<
  Record<TSlot, string | string[]>
>;

export type StringLike<TValue> = TValue extends string
  ? { kind: "text"; value: TValue }
  : { kind: "other"; value: TValue };

export type SelectionState<TItem extends KeyedItem> = {
  selected?: TItem["id"];
  highlighted?: TItem;
  byId?: Record<TItem["id"], TItem>;
};

export type RenderPayload<TItem extends KeyedItem, TValue> = {
  item: TItem;
  value: TValue;
  active: boolean;
  attrs?: {
    role: "option";
    tabindex: 0 | -1;
  };
};

export type SlotContract<TItem extends KeyedItem, TValue> = {
  default?: (payload: RenderPayload<TItem, TValue>) => unknown;
  empty?: () => null;
};

export type ComponentSurface<TValue, TItem extends KeyedItem = KeyedItem> = {
  modelValue?: TValue;
  defaultValue?: NonNullable<TValue>;
  items?: TItem[];
  variant?: ColorToken | "soft";
  ui?: StyleConfig;
  config?: Pick<ExternalSettings<TItem>, "size" | "tone" | "items">;
  passthrough?: Omit<ExternalSettings<TItem>, "debugOnly">;
  slots?: SlotContract<TItem, TValue>;
  state?: SelectionState<TItem>;
  labelFor?: (item: TItem, index: number) => string;
  status?: StringLike<TValue>;
};

export interface ConcreteItem extends KeyedItem {
  meta: {
    created: string;
    priority: 1 | 2 | 3;
  };
}

export type ConcreteSurface = ComponentSurface<string, ConcreteItem>;
export type ConfigSubset = Pick<ExternalSettings<ConcreteItem>, "size" | "tone" | "items">;
export type PassthroughSettings = Omit<ExternalSettings<ConcreteItem>, "debugOnly">;
export type StatusForString = ComponentSurface<string, ConcreteItem>["status"];
export type StatusForNumber = ComponentSurface<number, ConcreteItem>["status"];
export type SlotPayload = RenderPayload<ConcreteItem, string>;
export type SlotPayloadFromDefault = Parameters<
  NonNullable<NonNullable<ConcreteSurface["slots"]>["default"]>
>[0];
export type StyleForRoot = NonNullable<ConcreteSurface["ui"]>["root"];
