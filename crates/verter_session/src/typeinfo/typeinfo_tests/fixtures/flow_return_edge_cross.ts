// @ai-generated - Synthetic package-subpath flow-return edge fixture.

import { edgeAssertReady, edgeGetMap, edgeMaybe, edgePick } from "synthetic-edge-values/tools";

export function xf17() {
  return edgeGetMap().get("id")?.id;
}
export type XF17 = ReturnType<typeof xf17>;

export function xf18(input: unknown) {
  edgeAssertReady(input);
  return input.payload;
}
export type XF18 = ReturnType<typeof xf18>;

export function xf19() {
  return edgeMaybe({ id: "x" as const })?.id;
}
export type XF19 = ReturnType<typeof xf19>;

export function xf20() {
  return edgePick("left").value;
}
export type XF20 = ReturnType<typeof xf20>;

export function xf21(input: unknown) {
  edgeAssertReady(input);
  return edgeMaybe(edgePick("left"))?.value ?? input.payload;
}
export type XF21 = ReturnType<typeof xf21>;
