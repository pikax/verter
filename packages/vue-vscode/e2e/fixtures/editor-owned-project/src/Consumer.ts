import Comp from "./Comp.vue";

export const comp = Comp;

type CompProps = InstanceType<typeof Comp>["$props"];
export const validProps: CompProps = { label: "ready", count: 1 };
// The directive must be consumed. If the imported component surface regresses
// to `any`, TypeScript reports TS2578 and the editor acceptance fails.
// @ts-expect-error count is declared as number by LabelProps
const invalidProps: CompProps = { label: "broken", count: "not-a-number" };
void invalidProps;

export function useComp(): typeof Comp {
  return comp;
}
