// Fixed workload fixture. A second shared module so an edit to one shared
// dependency invalidates a different reverse-dependency set than an edit
// to props-base.ts. Frozen bytes; SHA-256 pinned by the budget catalog.

import type { Density } from "./props-base";

export interface Theme {
  density: Density;
  accent: string;
  spacing: Record<Density, number>;
}

export type ThemeSlice<K extends keyof Theme> = Pick<Theme, K>;

export const defaultTheme: Theme = {
  density: "cozy",
  accent: "#3b82f6",
  spacing: { compact: 4, cozy: 8, comfortable: 12 },
};
