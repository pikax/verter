export interface TableColumn<T> {
  key: keyof T & string;
  label: string;
  sortable?: boolean;
}

export interface TableRowClick<T> {
  row: T;
  index: number;
}
