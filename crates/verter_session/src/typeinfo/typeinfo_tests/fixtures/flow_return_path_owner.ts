// @ai-generated - Synthetic owner for path-precise flow-return coverage.

import type {
  AlternateDeep,
  AlternateEnvelope,
  SelectedDeep,
  SelectedEnvelope,
  UnusedEnvelope,
} from "./flow_return_path_barrel";
import {
  assertSelectedRecord,
  isSelectedReady,
  makeAlternate,
  makeSelected,
  selectedMap,
} from "./flow_return_path_barrel";
import * as pathApi from "./flow_return_path_barrel";

export type FlowPathSurface = {
  selected: SelectedDeep;
  alternate: AlternateDeep;
  unused: UnusedEnvelope;
  local: {
    ok: true;
  };
};

export function fp01(input: SelectedEnvelope) {
  return input.profile.name;
}
export type FP01 = ReturnType<typeof fp01>;

export function fp02(flag: boolean) {
  if (flag) return makeSelected("name").profile.name;
  return makeAlternate(1).stats.count;
}
export type FP02 = ReturnType<typeof fp02>;

export function fp03(input: unknown) {
  if (isSelectedReady(input)) return input.profile.nested.id;
  return undefined;
}
export type FP03 = ReturnType<typeof fp03>;

export function fp04(input: unknown) {
  assertSelectedRecord(input);
  return selectedMap(input.profile, (profile) => ({
    name: profile.name,
    id: profile.nested.id,
  })).id;
}
export type FP04 = ReturnType<typeof fp04>;

export function fp05(input: SelectedEnvelope | AlternateEnvelope) {
  if ("profile" in input) return input.profile.name;
  return input.stats.count;
}
export type FP05 = ReturnType<typeof fp05>;

export function fp06(input: Pick<FlowPathSurface, "selected">) {
  return input.selected.selected.profile.nested.id;
}
export type FP06 = ReturnType<typeof fp06>;

export function fp07(
  input:
    | {
        kind: "selected";
        value: SelectedEnvelope;
      }
    | {
        kind: "alternate";
        value: AlternateEnvelope;
      },
) {
  switch (input.kind) {
    case "selected":
      return input.value.profile.name;
    case "alternate":
      return input.value.stats.count;
  }
}
export type FP07 = ReturnType<typeof fp07>;

export function fp08(input: unknown, labels: string[]) {
  assertSelectedRecord(input);
  const found = labels.find((label): label is string => label.length > 0);
  return selectedMap(input.profile, (profile) => ({
    id: profile.nested.id,
    label: found ?? profile.name,
  }));
}
export type FP08 = ReturnType<typeof fp08>;

export function fp09(input: Pick<FlowPathSurface, "alternate">) {
  return input.alternate.alternate.stats.nested.code;
}
export type FP09 = ReturnType<typeof fp09>;

export function fp10(flag: boolean) {
  const local = { id: "local" as const };
  if (flag) return local.id;
  return undefined;
}
export type FP10 = ReturnType<typeof fp10>;

export function fp11() {
  return pathApi.makeSelected("namespace").profile.name;
}
export type FP11 = ReturnType<typeof fp11>;

export type FPKitchenInput =
  | {
      kind: "selected";
      payload?: SelectedEnvelope;
      labels?: string[];
    }
  | {
      kind: "alternate";
      payload: AlternateEnvelope;
      labels?: string[];
    };

export function fpKitchen(input: FPKitchenInput) {
  if (input.kind === "selected") {
    const payload = input.payload ?? makeSelected("fallback");
    assertSelectedRecord(payload);
    const found = input.labels?.find((label): label is string => label.length > 0);
    return selectedMap(payload.profile, (profile) => ({
      kind: "selected" as const,
      name: profile.name,
      found,
    }));
  }

  const alternate = makeAlternate(input.payload.stats.count);
  return {
    kind: "alternate" as const,
    count: alternate.stats.count,
  };
}
export type FPKitchen = ReturnType<typeof fpKitchen>;
