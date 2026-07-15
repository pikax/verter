// @ai-generated - Synthetic nested Parameters/NonNullable/indexed-access fixture.

export type SubmitPayload = {
  id: string;
  valid: boolean;
  meta?: {
    source: "keyboard" | "pointer";
  };
};

export interface HandlerSource {
  slots?: {
    submit?: ((payload: SubmitPayload, index: number) => unknown) | null;
    cancel?: (() => void) | null;
  } | null;
  items?: Array<{ id: string; value: number }> | null;
}

export type DirectParametersPayload = Parameters<
  (payload: SubmitPayload, index: number) => unknown
>[0];

export type DirectParametersTuple = Parameters<(payload: SubmitPayload, index: number) => unknown>;

export type DirectParametersSecond = Parameters<
  (payload: SubmitPayload, index: number) => unknown
>[1];

export type DirectReturnPayload = ReturnType<
  (payload: SubmitPayload) => { submitted: SubmitPayload; status: "ok" }
>;

export type NestedSubmitPayload = Parameters<
  NonNullable<NonNullable<NonNullable<HandlerSource["slots"]>["submit"]>>
>[0];

export type NestedFirstItem = NonNullable<NonNullable<HandlerSource["items"]>[number]>;

export type NestedCancelHandler = NonNullable<
  NonNullable<NonNullable<HandlerSource["slots"]>["cancel"]>
>;

export type NestedIndexedUtilitySurface = {
  submitPayload: NestedSubmitPayload;
  firstItem: NestedFirstItem;
  cancel: NestedCancelHandler;
};
