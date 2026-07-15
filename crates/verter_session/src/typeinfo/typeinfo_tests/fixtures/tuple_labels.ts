// @ai-generated - Synthetic tuple-label typeinfo fixture.

export type Handler = (name: string, count: number, active?: boolean) => void;

export type HandlerParams = Parameters<Handler>;
export type HandlerFirstParam = HandlerParams[0];
export type HandlerSecondParam = HandlerParams[1];
export type HandlerThirdParam = HandlerParams[2];
export type HandlerNumberElement = HandlerParams[number];

// Named tuple type alias used for direct projection without going through
// Parameters<>. Verter must preserve the label metadata when the tuple is the
// projection root.
export type DirectLabelledTuple = [first: string, second: number, third?: boolean];
