/**
 * Shared corpus types — vendored locally so `compileScript` imported-type
 * resolution never needs to read outside `corpus/` (the generator's hermetic
 * `fs` guard denies any such read).
 */
export interface Item {
  id: number;
  name: string;
}
