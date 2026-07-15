// @ai-generated - Synthetic cross-file flow-return owner fixture.

import type { User } from "./flow_return_cross_types";
import * as api from "./flow_return_cross_factory";
import { createConfig } from "./flow_return_cross_factory";
import { makeDual } from "./flow_return_cross_factory";
import { isNumber } from "./flow_return_cross_guards";
import { assertUser } from "./flow_return_cross_guards";
import { make } from "./flow_return_cross_index";
import { assertNumber, createDefaults, isLabel } from "./flow_return_cross_index";

export function xf01(user: User) {
  return user.id;
}
export type XF01 = ReturnType<typeof xf01>;

export function xf02() {
  return createConfig();
}
export type XF02 = ReturnType<typeof xf02>;

export function xf03(x: unknown) {
  if (isNumber(x)) return x;
  return 0 as const;
}
export type XF03 = ReturnType<typeof xf03>;

export function xf04() {
  return make();
}
export type XF04 = ReturnType<typeof xf04>;

export function xf05() {
  return api.makeOk();
}
export type XF05 = ReturnType<typeof xf05>;

export function xf06() {
  return makeDual();
}
export type XF06 = ReturnType<typeof xf06>;

export function xf07(x: unknown) {
  if (isLabel(x)) return x;
  return "";
}
export type XF07 = ReturnType<typeof xf07>;

export function xf08(x: unknown) {
  assertUser(x);
  return x.id;
}
export type XF08 = ReturnType<typeof xf08>;

export function xf09A(flag: boolean): number | string {
  return flag ? 1 : xf09B(flag);
}
export function xf09B(flag: boolean): number | string {
  return flag ? "b" : xf09A(flag);
}
export type XF09 = ReturnType<typeof xf09A>;

export declare function isReady(x: unknown): x is { ready: true };
export function xf11(x: unknown) {
  if (isReady(x)) return x.ready;
  return false;
}
export type XF11 = ReturnType<typeof xf11>;

export function vv15() {
  return createDefaults();
}
export type VV15 = ReturnType<typeof vv15>;

export function pa08FromBarrel(x: unknown) {
  assertNumber(x);
  return x;
}
export type PA08 = ReturnType<typeof pa08FromBarrel>;
