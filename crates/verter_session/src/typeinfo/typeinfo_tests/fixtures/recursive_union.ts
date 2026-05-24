// @ai-generated - Synthetic recursive, cyclic, union, and intersection fixture.

export interface TreeNode {
  id: string;
  children?: TreeNode[];
  meta?: RecursiveMeta;
}

export type RecursiveMeta = {
  depth: number;
  parent?: TreeNode;
};

export type AliasA = {
  kind: "a";
  next?: AliasB;
};

export type AliasB = {
  kind: "b";
  previous?: AliasA;
};

export type RecursiveTreeSurface = TreeNode;
export type CyclicAliasSurface = AliasA;

export type IntersectionContribution = { id: string } & {
  label: string;
} & {
  count: number;
  ready?: boolean;
};

export type ObjectUnionContribution =
  | { variant: "text"; text: string; shared: boolean }
  | { variant: "count"; count: number; shared: boolean };
