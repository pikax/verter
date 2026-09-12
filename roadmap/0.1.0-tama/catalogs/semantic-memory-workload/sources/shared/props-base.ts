// Fixed workload fixture. Cross-file type surface every carrier fixture
// imports from, so the workload exercises cross-file resolution rather
// than N isolated single-file requests.
//
// These bytes are frozen: catalogs/semantic-memory-budget.toml pins this
// file's SHA-256. Editing it without re-pinning fails budget validation.

export type Density = "compact" | "cozy" | "comfortable";

export interface Identified {
  id: string;
}

export interface Labelled extends Identified {
  label: string;
  hint?: string;
}

export interface PropsBase<T extends Identified> {
  rows: readonly T[];
  density: Density;
  selected?: T | null;
}

export type Selection<T extends Identified> = Pick<PropsBase<T>, "selected">;

export type Emitted<T extends Identified> = {
  select: [row: T];
  clear: [];
};
