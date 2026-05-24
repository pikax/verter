// @ai-generated - Synthetic expansion-boundary owner fixture.

import type { SelectedBranch } from "./expansion_selected";
import type { UnselectedBranch } from "./expansion_unselected";

export type LocalPayload = {
  id: string;
  meta: {
    rank: number;
    hidden: {
      secret: boolean;
    };
  };
  branch: {
    visible: "local";
    deep: {
      token: "deep";
    };
  };
};

export type ExpansionSurface = {
  local: LocalPayload;
  inline: {
    visible: boolean;
    details: {
      note: string;
      count: number;
    };
  };
  selected: SelectedBranch;
  unused: UnselectedBranch;
};

export type PickedExpansion = Pick<ExpansionSurface, "local" | "inline">;
export type OmittedExpansion = Omit<ExpansionSurface, "unused">;
export type InlineDetailsProjection = ExpansionSurface["inline"]["details"];
export type LocalBranchProjection = ExpansionSurface["local"]["branch"];
export type ImportedSelectedProjection = ExpansionSurface["selected"]["selected"];
export type ImportedNestedFlagProjection =
  ExpansionSurface["selected"]["selected"]["nested"]["flag"];
