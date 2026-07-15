export interface ToolbarItem {
  id: string;
  label: string;
  icon?: string;
}

export type ToolbarItemOrGroup = ToolbarItem | ToolbarItem[];
