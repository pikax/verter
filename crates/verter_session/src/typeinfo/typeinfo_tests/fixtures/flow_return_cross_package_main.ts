// @ai-generated - Synthetic package-backed flow-return owner fixture.

import {
  xf10GetValue,
  xf12GetPair,
  xf13IsRecord,
  xf14AssertRecord,
  xf15Wrap,
  xf16Pick,
} from "synthetic-flow-values";

export function xf10() {
  return xf10GetValue().id;
}
export type XF10 = ReturnType<typeof xf10>;

export function xf12() {
  return xf12GetPair()[0].id;
}
export type XF12 = ReturnType<typeof xf12>;

export function xf13(x: unknown) {
  if (xf13IsRecord(x)) return x.label;
  return "";
}
export type XF13 = ReturnType<typeof xf13>;

export function xf14(x: unknown) {
  xf14AssertRecord(x);
  return x.label;
}
export type XF14 = ReturnType<typeof xf14>;

export function xf15() {
  return xf15Wrap("wrapped" as const).value;
}
export type XF15 = ReturnType<typeof xf15>;

export function xf16() {
  return xf16Pick("count");
}
export type XF16 = ReturnType<typeof xf16>;
