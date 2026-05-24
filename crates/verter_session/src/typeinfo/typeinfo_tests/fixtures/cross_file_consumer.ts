// @ai-generated - Synthetic renamed-import consumer typeinfo fixture.

import type {
  RenamedItem,
  RenamedSurface,
  RemoteStyle as StyleAlias,
} from "/fixtures/cross-file-barrel";

export interface LocalItem extends RenamedItem {
  extra: number;
}

export type CrossFileSurface = RenamedSurface<LocalItem> & {
  ui?: StyleAlias;
};

export type CrossFileProjectedItem = CrossFileSurface["item"];
export type CrossFileProjectedExtra = CrossFileSurface["item"]["extra"];
export type CrossFileLabelFirstParam = Parameters<NonNullable<CrossFileSurface["labelFor"]>>[0];
