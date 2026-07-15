// @ai-generated - Synthetic menu-like typeinfo fixture.

export type VNode = unknown;
export type AcceptableValue = string | number | boolean | null | { id: string };
export type ArrayOrNested<T> = T[] | T[][];
export type NestedItem<A> = A extends readonly (infer I)[]
  ? I extends readonly (infer Inner)[]
    ? Inner
    : I
  : never;
export type GetItemKeys<T> = Extract<keyof NestedItem<T>, string>;
export type GetItemValue<T, VK extends GetItemKeys<T> | undefined> =
  VK extends GetItemKeys<T> ? NestedItem<T>[VK] : NestedItem<T>;
export type GetModelValue<
  T,
  VK extends GetItemKeys<T> | undefined,
  M extends boolean,
  Excluded,
> = M extends true
  ? Exclude<GetItemValue<T, VK>, Excluded>[]
  : Exclude<GetItemValue<T, VK>, Excluded>;
export type ModelModifiers = { trim?: true; number?: true; nullable?: true; lazy?: true };
export type ApplyModifiers<T, Mod> = Mod extends { number: true }
  ? number
  : Mod extends { trim: true }
    ? T extends string
      ? string
      : T
    : T;
export type LinkKeys = "href" | "target" | "rel";
export interface ButtonLikeProps {
  label?: string;
  icon?: string;
  href?: string;
  target?: string;
  rel?: string;
  onClick?: (event: Event) => void;
}
export interface MenuUi {
  item?: string;
  itemLabel?: string;
  itemTrailing?: string;
  content?: string;
}
export type ExcludeItem = { type: "label" | "separator" };
export type IsClearUsed<M extends boolean, C extends boolean | object> = M extends false
  ? C extends true
    ? null
    : C extends object
      ? null
      : never
  : never;
export interface MenuItem {
  id: string;
  label?: string;
  description?: string;
  type?: "label" | "separator" | "item";
  disabled?: boolean;
  onSelect?: (event: Event) => void;
  clear?: boolean | Partial<Omit<ButtonLikeProps, LinkKeys>>;
  ui?: Pick<MenuUi, "item" | "itemLabel" | "itemTrailing">;
}
export interface MenuProps<
  A extends ArrayOrNested<MenuItem>,
  VK extends GetItemKeys<A> | undefined = undefined,
  M extends boolean = false,
  Mod extends Omit<ModelModifiers, "lazy"> = Omit<ModelModifiers, "lazy">,
  C extends boolean | object = false,
> {
  items?: A;
  valueKey?: VK;
  multiple?: M & boolean;
  clear?: (C & boolean) | (C & Partial<Omit<ButtonLikeProps, LinkKeys>>);
  defaultValue?: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>;
  modelValue?: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>;
  ui?: MenuUi;
}
export interface MenuSlots<
  A extends ArrayOrNested<MenuItem>,
  VK extends GetItemKeys<A> | undefined = undefined,
  M extends boolean = false,
  Mod extends Omit<ModelModifiers, "lazy"> = Omit<ModelModifiers, "lazy">,
  C extends boolean | object = false,
  T extends NestedItem<A> = NestedItem<A>,
> {
  leading?(props: {
    modelValue: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>;
    open: boolean;
    ui: MenuUi;
  }): VNode[];
  trailing?(props: {
    modelValue: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>;
    open: boolean;
    ui: MenuUi;
  }): VNode[];
  item?: (props: { item: T; index: number; ui: MenuUi }) => VNode[];
}
export type ConcreteMenuItems = MenuItem[];
export type ConcreteMenuPropsSurface = MenuProps<
  ConcreteMenuItems,
  "id",
  true,
  { trim: true },
  true
>;
export type ConcreteMenuModelValue = NonNullable<
  MenuProps<ConcreteMenuItems, "id", true, { trim: true }, true>["modelValue"]
>;
export type ConcreteMenuLeadingSlotPayload = Parameters<
  NonNullable<MenuSlots<ConcreteMenuItems, "id", true, { trim: true }, true>["leading"]>
>[0];
