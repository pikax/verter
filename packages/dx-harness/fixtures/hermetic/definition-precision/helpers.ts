// A tiny imported module so go-to-definition has a cross-file target whose exact
// declaration line (never line 0) the scenario pins.

export interface PanelConfig {
  title: string;
  closable: boolean;
}

export function makeConfig(title: string): PanelConfig {
  return { title, closable: true };
}
