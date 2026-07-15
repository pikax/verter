// @ai-generated - Synthetic selected expansion-boundary fixture.

export type SelectedLeaf = {
  value: string;
  nested: {
    flag: boolean;
    deep: {
      code: "selected";
    };
  };
};

export type SelectedBranch = {
  selected: SelectedLeaf;
  internalOnly: {
    secret: number;
  };
};

export type SelectedExtra = {
  shouldNotLoad: "extra";
};
