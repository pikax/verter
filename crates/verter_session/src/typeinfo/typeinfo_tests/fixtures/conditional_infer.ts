// @ai-generated - Synthetic conditional and infer typeinfo fixture.

export interface ActionPayload {
  kind: "action";
  id: string;
  meta?: {
    priority: 1 | 2 | 3;
    tags?: string[];
  };
}

export type ArrayElement<T> = T extends readonly (infer Item)[] ? Item : never;

export type DeepCurrent<T> = T extends { data: { current: infer Current } }
  ? NonNullable<Current>
  : never;

export type FirstParameter<T> = T extends (payload: infer Payload, index: number) => unknown
  ? Payload
  : never;

export type ElementStatus<T> =
  ArrayElement<T> extends { kind: infer Kind }
    ? Kind extends string
      ? { kind: Kind; item: ArrayElement<T> }
      : never
    : never;

export type TuplePair<T> = T extends readonly [infer Head, infer Tail]
  ? { head: Head; tail: Tail }
  : never;

export type FunctionResult<T> = T extends (payload: ActionPayload, index: number) => infer Result
  ? Result
  : never;

export type ConcreteArrayItem = ArrayElement<ActionPayload[]>;
export type ConcreteDeepCurrent = DeepCurrent<{ data: { current: ActionPayload | null } }>;
export type ConcreteFirstParameter = FirstParameter<
  (payload: ActionPayload, index: number) => void
>;
export type ConcreteElementStatus = ElementStatus<ActionPayload[]>;
export type ConcreteTuplePair = TuplePair<readonly [ActionPayload, { count: number }]>;
export type ConcreteFunctionResult = FunctionResult<
  (payload: ActionPayload, index: number) => { ok: true; payload: ActionPayload }
>;

export type ConditionalInferSurface = {
  item: ConcreteArrayItem;
  current: ConcreteDeepCurrent;
  callbackPayload: ConcreteFirstParameter;
  status: ConcreteElementStatus;
  tuplePair: ConcreteTuplePair;
  functionResult: ConcreteFunctionResult;
};
