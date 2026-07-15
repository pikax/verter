// @ai-generated - Synthetic table-like typeinfo fixture.

export type VNode = unknown;
export type Updater<T> = T | ((old: T) => T);
export interface Row<T> {
  original: T;
  id: string;
  getValue<K extends keyof T>(key: K): T[K];
}
export interface Cell<T, V> {
  row: Row<T>;
  value: V;
}
export interface Header<T, V> {
  id: string;
  column: Column<T, V>;
}
export interface HeaderContext<T, V> {
  row: Row<T>;
  header: Header<T, V>;
}
export interface CellContext<T, V> {
  row: Row<T>;
  cell: Cell<T, V>;
}
export interface Column<T, V> {
  id: string;
  accessor: keyof T;
  meta?: ColumnMeta<T, V>;
}
export type ColumnDef<T, V = unknown> = {
  accessorKey?: keyof T;
  header?: string | ((ctx: HeaderContext<T, V>) => VNode);
  cell?: (ctx: CellContext<T, V>) => VNode;
  columns?: ColumnDef<T, V>[];
  meta?: ColumnMeta<T, V>;
};
export interface ColumnMeta<T, V> {
  class?: {
    th?: string | ((cell: Header<T, V>) => string);
    td?: string | ((cell: Cell<T, V>) => string);
  };
  style?: {
    th?:
      | string
      | Record<string, string>
      | ((cell: Header<T, V>) => string | Record<string, string>);
    td?: string | Record<string, string> | ((cell: Cell<T, V>) => string | Record<string, string>);
  };
}
export interface CoreOptions<T> {
  data: T[];
  columns: ColumnDef<T>[];
  state?: Record<string, unknown>;
  onStateChange?: (updater: Updater<Record<string, unknown>>) => void;
  renderFallbackValue?: unknown;
  getRowId?: (row: T, index: number) => string;
}
export interface VirtualizerOptions<TScroll, TItem> {
  getScrollElement: () => TScroll | null;
  count: number;
  estimateSize: number | ((index: number) => number);
  overscan: number;
  measureElement?: (el: TItem) => number;
}

export interface GridRow {
  col00: { label: string; value: number; nested: { token: "c00" } };
  col01: { label: string; value: number; nested: { token: "c01" } };
  col02: { label: string; value: number; nested: { token: "c02" } };
  col03: { label: string; value: number; nested: { token: "c03" } };
  col04: { label: string; value: number; nested: { token: "c04" } };
  col05: { label: string; value: number; nested: { token: "c05" } };
  col06: { label: string; value: number; nested: { token: "c06" } };
  col07: { label: string; value: number; nested: { token: "c07" } };
  col08: { label: string; value: number; nested: { token: "c08" } };
  col09: { label: string; value: number; nested: { token: "c09" } };
  col10: { label: string; value: number; nested: { token: "c10" } };
  col11: { label: string; value: number; nested: { token: "c11" } };
}

export interface Feature00State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature00Options<T> {
  state00?: Feature00State;
  onFeature00Change?: (updater: Updater<Feature00State>) => void;
  getFeature00Model?: () => Row<T>[];
  feature00Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature00State) => string | Record<string, string>;
  };
}

export interface Feature01State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature01Options<T> {
  state01?: Feature01State;
  onFeature01Change?: (updater: Updater<Feature01State>) => void;
  getFeature01Model?: () => Row<T>[];
  feature01Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature01State) => string | Record<string, string>;
  };
}

export interface Feature02State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature02Options<T> {
  state02?: Feature02State;
  onFeature02Change?: (updater: Updater<Feature02State>) => void;
  getFeature02Model?: () => Row<T>[];
  feature02Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature02State) => string | Record<string, string>;
  };
}

export interface Feature03State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature03Options<T> {
  state03?: Feature03State;
  onFeature03Change?: (updater: Updater<Feature03State>) => void;
  getFeature03Model?: () => Row<T>[];
  feature03Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature03State) => string | Record<string, string>;
  };
}

export interface Feature04State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature04Options<T> {
  state04?: Feature04State;
  onFeature04Change?: (updater: Updater<Feature04State>) => void;
  getFeature04Model?: () => Row<T>[];
  feature04Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature04State) => string | Record<string, string>;
  };
}

export interface Feature05State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature05Options<T> {
  state05?: Feature05State;
  onFeature05Change?: (updater: Updater<Feature05State>) => void;
  getFeature05Model?: () => Row<T>[];
  feature05Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature05State) => string | Record<string, string>;
  };
}

export interface Feature06State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature06Options<T> {
  state06?: Feature06State;
  onFeature06Change?: (updater: Updater<Feature06State>) => void;
  getFeature06Model?: () => Row<T>[];
  feature06Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature06State) => string | Record<string, string>;
  };
}

export interface Feature07State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature07Options<T> {
  state07?: Feature07State;
  onFeature07Change?: (updater: Updater<Feature07State>) => void;
  getFeature07Model?: () => Row<T>[];
  feature07Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature07State) => string | Record<string, string>;
  };
}

export interface Feature08State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature08Options<T> {
  state08?: Feature08State;
  onFeature08Change?: (updater: Updater<Feature08State>) => void;
  getFeature08Model?: () => Row<T>[];
  feature08Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature08State) => string | Record<string, string>;
  };
}

export interface Feature09State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature09Options<T> {
  state09?: Feature09State;
  onFeature09Change?: (updater: Updater<Feature09State>) => void;
  getFeature09Model?: () => Row<T>[];
  feature09Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature09State) => string | Record<string, string>;
  };
}

export interface Feature10State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature10Options<T> {
  state10?: Feature10State;
  onFeature10Change?: (updater: Updater<Feature10State>) => void;
  getFeature10Model?: () => Row<T>[];
  feature10Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature10State) => string | Record<string, string>;
  };
}

export interface Feature11State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature11Options<T> {
  state11?: Feature11State;
  onFeature11Change?: (updater: Updater<Feature11State>) => void;
  getFeature11Model?: () => Row<T>[];
  feature11Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature11State) => string | Record<string, string>;
  };
}

export interface Feature12State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature12Options<T> {
  state12?: Feature12State;
  onFeature12Change?: (updater: Updater<Feature12State>) => void;
  getFeature12Model?: () => Row<T>[];
  feature12Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature12State) => string | Record<string, string>;
  };
}

export interface Feature13State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature13Options<T> {
  state13?: Feature13State;
  onFeature13Change?: (updater: Updater<Feature13State>) => void;
  getFeature13Model?: () => Row<T>[];
  feature13Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature13State) => string | Record<string, string>;
  };
}

export interface Feature14State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature14Options<T> {
  state14?: Feature14State;
  onFeature14Change?: (updater: Updater<Feature14State>) => void;
  getFeature14Model?: () => Row<T>[];
  feature14Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature14State) => string | Record<string, string>;
  };
}

export interface Feature15State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature15Options<T> {
  state15?: Feature15State;
  onFeature15Change?: (updater: Updater<Feature15State>) => void;
  getFeature15Model?: () => Row<T>[];
  feature15Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature15State) => string | Record<string, string>;
  };
}

export interface Feature16State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature16Options<T> {
  state16?: Feature16State;
  onFeature16Change?: (updater: Updater<Feature16State>) => void;
  getFeature16Model?: () => Row<T>[];
  feature16Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature16State) => string | Record<string, string>;
  };
}

export interface Feature17State {
  enabled?: boolean;
  keys?: Array<keyof GridRow>;
  payload?: { id: string; rows: Row<GridRow>[]; meta: Record<string, string> };
}
export interface Feature17Options<T> {
  state17?: Feature17State;
  onFeature17Change?: (updater: Updater<Feature17State>) => void;
  getFeature17Model?: () => Row<T>[];
  feature17Meta?: {
    column?: ColumnDef<T>;
    resolve?: (row: Row<T>, state: Feature17State) => string | Record<string, string>;
  };
}

export interface GridOptions<T> extends Omit<
  CoreOptions<T>,
  "data" | "columns" | "onStateChange" | "renderFallbackValue"
> {
  state?: CoreOptions<T>["state"];
  onStateChange?: CoreOptions<T>["onStateChange"];
  renderFallbackValue?: CoreOptions<T>["renderFallbackValue"];
}
export interface GridProps<T = GridRow> extends GridOptions<T> {
  data?: T[];
  columns?: ColumnDef<T>[];
  virtualize?:
    | boolean
    | (Partial<
        Omit<
          VirtualizerOptions<Element, Element>,
          "getScrollElement" | "count" | "estimateSize" | "overscan"
        >
      > & { overscan?: number; estimateSize?: number | ((index: number) => number) });
  onSelect?: (event: Event, row: Row<T>) => void;
  feature00Options?: Omit<Feature00Options<T>, "onFeature00Change">;
  feature01Options?: Omit<Feature01Options<T>, "onFeature01Change">;
  feature02Options?: Omit<Feature02Options<T>, "onFeature02Change">;
  feature03Options?: Omit<Feature03Options<T>, "onFeature03Change">;
  feature04Options?: Omit<Feature04Options<T>, "onFeature04Change">;
  feature05Options?: Omit<Feature05Options<T>, "onFeature05Change">;
  feature06Options?: Omit<Feature06Options<T>, "onFeature06Change">;
  feature07Options?: Omit<Feature07Options<T>, "onFeature07Change">;
  feature08Options?: Omit<Feature08Options<T>, "onFeature08Change">;
  feature09Options?: Omit<Feature09Options<T>, "onFeature09Change">;
  feature10Options?: Omit<Feature10Options<T>, "onFeature10Change">;
  feature11Options?: Omit<Feature11Options<T>, "onFeature11Change">;
  feature12Options?: Omit<Feature12Options<T>, "onFeature12Change">;
  feature13Options?: Omit<Feature13Options<T>, "onFeature13Change">;
  feature14Options?: Omit<Feature14Options<T>, "onFeature14Change">;
  feature15Options?: Omit<Feature15Options<T>, "onFeature15Change">;
  feature16Options?: Omit<Feature16Options<T>, "onFeature16Change">;
  feature17Options?: Omit<Feature17Options<T>, "onFeature17Change">;
}
export type DynamicHeaderFooterSlots<T, K = keyof T> = Record<
  | `${K extends string ? K : never}-header`
  | `${K extends string ? K : never}-footer`
  | (string & {}),
  (props: HeaderContext<T, unknown>) => VNode[]
>;
export type DynamicCellSlots<T, K = keyof T> = Record<
  `${K extends string ? K : never}-cell` | (string & {}),
  (props: CellContext<T, unknown>) => VNode[]
>;
export type GridSlots<T = GridRow> = {
  empty?: (props?: {}) => VNode[];
  loading?: (props?: {}) => VNode[];
  expanded?: (props: { row: Row<T> }) => VNode[];
} & DynamicHeaderFooterSlots<T> &
  DynamicCellSlots<T>;
export type ConcreteGridProps = GridProps<GridRow>;
export type NameCellSlot = NonNullable<GridSlots<GridRow>["col00-cell"]>;
