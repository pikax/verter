// @ai-generated - Synthetic cross-file leaf typeinfo fixture.

export interface RemoteItem {
  id: string;
  label: string;
  flag?: boolean;
}

export type RemoteStyle = {
  root?: string;
  item?: string;
};

export type RemoteSurface<TItem extends RemoteItem = RemoteItem> = {
  item: TItem;
  items: TItem[];
  ui?: RemoteStyle;
  labelFor?: (item: TItem, index: number) => string;
};
