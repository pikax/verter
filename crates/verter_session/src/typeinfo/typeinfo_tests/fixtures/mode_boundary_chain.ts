// @ai-generated - 12-level deep-chain fixture for mode_boundary_invariants
// tests. Mirrors the tsgo-audit benchmark shape from
// `benchmarks/tsgo-audit/large-fixture/src/large-types.ts` (where the
// benchmark run observed `recursion_limit_reached=true` at 500 levels).
// The synthetic shape pins per-level Pick<parent>, payload value union,
// and an outer `LargeKeys_N = keyof LargeRecord_N` alias. Resolving
// `LargeKeys_11` (the terminal alias) must only need the SHALLOW member
// names of `LargeRecord_11`, NOT a full walk of the dependent chain.
//
// TS7 emission verified against tsgo 7.0.0-dev.20260523.1:
//   type LargeKeys_11 = keyof LargeRecord_11
//   = "id" | "tag" | "parent" | "payload"

export type LargeValue_0 = "value_0";
export interface LargeRecord_0 {
  id: 0;
  tag: "tag_0";
  payload: { value: LargeValue_0 };
}
export type LargeKeys_0 = keyof LargeRecord_0;

export type LargeValue_1 = LargeValue_0 | "value_1";
export interface LargeRecord_1 {
  id: 1;
  tag: "tag_1";
  parent: Pick<LargeRecord_0, "id" | "tag">;
  payload: { value: LargeValue_1 };
}
export type LargeKeys_1 = keyof LargeRecord_1;

export type LargeValue_2 = LargeValue_1 | "value_2";
export interface LargeRecord_2 {
  id: 2;
  tag: "tag_2";
  parent: Pick<LargeRecord_1, "id" | "tag">;
  payload: { value: LargeValue_2 };
}
export type LargeKeys_2 = keyof LargeRecord_2;

export type LargeValue_3 = LargeValue_2 | "value_3";
export interface LargeRecord_3 {
  id: 3;
  tag: "tag_3";
  parent: Pick<LargeRecord_2, "id" | "tag">;
  payload: { value: LargeValue_3 };
}
export type LargeKeys_3 = keyof LargeRecord_3;

export type LargeValue_4 = LargeValue_3 | "value_4";
export interface LargeRecord_4 {
  id: 4;
  tag: "tag_4";
  parent: Pick<LargeRecord_3, "id" | "tag">;
  payload: { value: LargeValue_4 };
}
export type LargeKeys_4 = keyof LargeRecord_4;

export type LargeValue_5 = LargeValue_4 | "value_5";
export interface LargeRecord_5 {
  id: 5;
  tag: "tag_5";
  parent: Pick<LargeRecord_4, "id" | "tag">;
  payload: { value: LargeValue_5 };
}
export type LargeKeys_5 = keyof LargeRecord_5;

export type LargeValue_6 = LargeValue_5 | "value_6";
export interface LargeRecord_6 {
  id: 6;
  tag: "tag_6";
  parent: Pick<LargeRecord_5, "id" | "tag">;
  payload: { value: LargeValue_6 };
}
export type LargeKeys_6 = keyof LargeRecord_6;

export type LargeValue_7 = LargeValue_6 | "value_7";
export interface LargeRecord_7 {
  id: 7;
  tag: "tag_7";
  parent: Pick<LargeRecord_6, "id" | "tag">;
  payload: { value: LargeValue_7 };
}
export type LargeKeys_7 = keyof LargeRecord_7;

export type LargeValue_8 = LargeValue_7 | "value_8";
export interface LargeRecord_8 {
  id: 8;
  tag: "tag_8";
  parent: Pick<LargeRecord_7, "id" | "tag">;
  payload: { value: LargeValue_8 };
}
export type LargeKeys_8 = keyof LargeRecord_8;

export type LargeValue_9 = LargeValue_8 | "value_9";
export interface LargeRecord_9 {
  id: 9;
  tag: "tag_9";
  parent: Pick<LargeRecord_8, "id" | "tag">;
  payload: { value: LargeValue_9 };
}
export type LargeKeys_9 = keyof LargeRecord_9;

export type LargeValue_10 = LargeValue_9 | "value_10";
export interface LargeRecord_10 {
  id: 10;
  tag: "tag_10";
  parent: Pick<LargeRecord_9, "id" | "tag">;
  payload: { value: LargeValue_10 };
}
export type LargeKeys_10 = keyof LargeRecord_10;

export type LargeValue_11 = LargeValue_10 | "value_11";
export interface LargeRecord_11 {
  id: 11;
  tag: "tag_11";
  parent: Pick<LargeRecord_10, "id" | "tag">;
  payload: { value: LargeValue_11 };
}
export type LargeKeys_11 = keyof LargeRecord_11;
