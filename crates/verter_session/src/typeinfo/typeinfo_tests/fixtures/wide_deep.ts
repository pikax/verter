// @ai-generated - Synthetic wide/deep component-like typeinfo fixture.

export type Token = "alpha" | "beta" | "gamma";

export interface Leaf {
  id: string;
  score: number;
  flags?: Partial<Record<"pinned" | "active", boolean>>;
}

export type Action = {
  id: string;
  label: string;
  disabled?: boolean;
};

export type WidePanel<TLeaf extends Leaf = Leaf> = {
  header: {
    title: string;
    actions?: Action[];
  };
  row00?: TLeaf;
  row01?: TLeaf;
  row02?: TLeaf;
  row03?: TLeaf;
  row04?: TLeaf;
  row05?: TLeaf;
  row06?: TLeaf;
  row07?: TLeaf;
  row08?: TLeaf;
  row09?: TLeaf;
  row10?: TLeaf;
  row11?: TLeaf;
  row12?: TLeaf;
  row13?: TLeaf;
  row14?: TLeaf;
  row15?: TLeaf;
  nested: {
    level1: {
      level2: {
        target: Pick<TLeaf, "id" | "score"> & {
          token: Token;
        };
      };
    };
  };
};

export type WideDeepSurface = WidePanel;
export type WideDeepProjectedTarget = WidePanel["nested"]["level1"]["level2"]["target"];
export type WideDeepProjectedToken = WidePanel["nested"]["level1"]["level2"]["target"]["token"];
export type WideDeepRowFlags = NonNullable<WidePanel["row00"]>["flags"];
export type WideDeepFlagActive = NonNullable<NonNullable<WidePanel["row00"]>["flags"]>["active"];
