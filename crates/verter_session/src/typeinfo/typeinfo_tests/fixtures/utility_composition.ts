// @ai-generated - Synthetic utility-composition typeinfo fixture.

export interface UtilitySource {
  id: string;
  label?: string;
  tone?: "neutral" | "accent" | "danger";
  mode: "view" | "edit" | "debug";
  internal?: {
    trace: boolean;
    sink: (event: string) => void;
  };
  payload?: {
    count?: number;
    tags?: string[];
  };
}

export type RequiredIdentity = Required<Pick<UtilitySource, "id" | "label">>;
export type PublicPartial = Partial<Omit<UtilitySource, "internal">>;
export type VisibleMode = Extract<UtilitySource["mode"], "view" | "edit">;
export type RuntimeMode = Exclude<UtilitySource["mode"], "debug">;
export type UtilityCombinationSurface = RequiredIdentity &
  PublicPartial & {
    visibleMode: VisibleMode;
    runtimeMode: RuntimeMode;
  };

export type DeepUtilityPayload = Required<
  Pick<NonNullable<UtilitySource["payload"]>, "count" | "tags">
>;

export type DeepUtilityConfig = Required<
  Pick<Partial<Omit<UtilitySource, "internal">>, "mode" | "payload">
> & {
  mode: Extract<UtilitySource["mode"], "view" | "edit">;
  tone: Exclude<NonNullable<UtilitySource["tone"]>, "danger">;
  payload: DeepUtilityPayload;
};
