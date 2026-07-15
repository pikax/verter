export interface Row {
  id: number;
}

export interface Events {
  (e: "save", value: Row): void;
}

export interface ImportedEmits {
  save: [id: number];
}
