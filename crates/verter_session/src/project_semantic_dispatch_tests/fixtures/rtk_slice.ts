type Exclude<T, U> = T extends U ? never : T;
type Pick<T, K extends keyof T> = { [P in K]: T[P] };
type Omit<T, K extends keyof any> = Pick<T, Exclude<keyof T, K>>;
type Record<K extends keyof any, T> = { [P in K]: T };
interface Action<T extends string = string> {
  type: T;
}
interface UnknownAction extends Action {
  [extraProps: string]: unknown;
}
type IsAny<T, True, False = never> = true | false extends (T extends never ? true : false)
  ? True
  : False;
type IsUnknown<T, True, False = never> = unknown extends T ? IsAny<T, False, True> : False;
type IfMaybeUndefined<P, True, False> = [undefined] extends [P] ? True : False;
type IfVoid<P, True, False> = [void] extends [P] ? True : False;
type IsEmptyObj<T, True, False = never> = T extends any
  ? keyof T extends never
    ? IsUnknown<T, False, IfMaybeUndefined<T, False, IfVoid<T, False, True>>>
    : False
  : never;
type AtLeastTS35<True, False> = [True, False][IsUnknown<ReturnType<<T>() => T>, 0, 1>];
type IsUnknownOrNonInferrable<T, True, False> = AtLeastTS35<
  IsUnknown<T, True, False>,
  IsEmptyObj<T, True, IsUnknown<T, True, False>>
>;
type PayloadAction<P = void, T extends string = string, M = never, E = never> = {
  payload: P;
  type: T;
} & ([M] extends [never] ? {} : { meta: M }) &
  ([E] extends [never] ? {} : { error: E });
type PrepareAction<P> =
  | ((...args: any[]) => { payload: P })
  | ((...args: any[]) => { payload: P; meta: any })
  | ((...args: any[]) => { payload: P; error: any })
  | ((...args: any[]) => { payload: P; meta: any; error: any });
type _ActionCreatorWithPreparedPayload<
  PA extends PrepareAction<any> | void,
  T extends string = string,
> =
  PA extends PrepareAction<infer P>
    ? ActionCreatorWithPreparedPayload<
        Parameters<PA>,
        P,
        T,
        ReturnType<PA> extends { error: infer E } ? E : never,
        ReturnType<PA> extends { meta: infer M } ? M : never
      >
    : void;
interface BaseActionCreator<P, T extends string, M = never, E = never> {
  type: T;
  match: (action: unknown) => action is PayloadAction<P, T, M, E>;
}
interface ActionCreatorWithPreparedPayload<
  Args extends unknown[],
  P,
  T extends string = string,
  E = never,
  M = never,
> extends BaseActionCreator<P, T, M, E> {
  (...args: Args): PayloadAction<P, T, M, E>;
}
interface ActionCreatorWithOptionalPayload<P, T extends string = string> extends BaseActionCreator<
  P,
  T
> {
  (payload?: P): PayloadAction<P, T>;
}
interface ActionCreatorWithoutPayload<T extends string = string> extends BaseActionCreator<
  undefined,
  T
> {
  (noArgument: void): PayloadAction<undefined, T>;
}
interface ActionCreatorWithPayload<P, T extends string = string> extends BaseActionCreator<P, T> {
  (payload: P): PayloadAction<P, T>;
}
interface ActionCreatorWithNonInferrablePayload<
  T extends string = string,
> extends BaseActionCreator<unknown, T> {
  <PT extends unknown>(payload: PT): PayloadAction<PT, T>;
}
type IfPrepareActionMethodProvided<PA extends PrepareAction<any> | void, True, False> = PA extends (
  ...args: any[]
) => any
  ? True
  : False;
type PayloadActionCreator<
  P = void,
  T extends string = string,
  PA extends PrepareAction<P> | void = void,
> = IfPrepareActionMethodProvided<
  PA,
  _ActionCreatorWithPreparedPayload<PA, T>,
  IsAny<
    P,
    ActionCreatorWithPayload<any, T>,
    IsUnknownOrNonInferrable<
      P,
      ActionCreatorWithNonInferrablePayload<T>,
      IfVoid<
        P,
        ActionCreatorWithoutPayload<T>,
        IfMaybeUndefined<P, ActionCreatorWithOptionalPayload<P, T>, ActionCreatorWithPayload<P, T>>
      >
    >
  >
>;
type CaseReducer<S = any, A extends Action = UnknownAction> = (state: S, action: A) => S | void;
type CaseReducerWithPrepare<State, Action extends PayloadAction> = {
  reducer: CaseReducer<State, Action>;
  prepare: PrepareAction<Action["payload"]>;
};
type SliceCaseReducers<State> = Record<
  string,
  | CaseReducer<State, PayloadAction<any>>
  | CaseReducerWithPrepare<State, PayloadAction<any, string, any, any>>
>;
type SliceActionType<SliceName extends string, ActionName extends keyof any> = ActionName extends
  | string
  | number
  ? `${SliceName}/${ActionName}`
  : string;
type ActionCreatorForCaseReducerWithPrepare<
  CR extends { prepare: any },
  Type extends string,
> = _ActionCreatorWithPreparedPayload<CR["prepare"], Type>;
type ActionCreatorForCaseReducer<CR, Type extends string> = CR extends (
  state: any,
  action: infer Action,
) => any
  ? Action extends { payload: infer P }
    ? PayloadActionCreator<P, Type>
    : ActionCreatorWithoutPayload<Type>
  : ActionCreatorWithoutPayload<Type>;
type CaseReducerActions<CaseReducers extends SliceCaseReducers<any>, SliceName extends string> = {
  [Type in keyof CaseReducers]: CaseReducers[Type] extends { prepare: any }
    ? ActionCreatorForCaseReducerWithPrepare<CaseReducers[Type], SliceActionType<SliceName, Type>>
    : ActionCreatorForCaseReducer<CaseReducers[Type], SliceActionType<SliceName, Type>>;
};
type ValidateSliceCaseReducers<S, ACR extends SliceCaseReducers<S>> = ACR & {
  [T in keyof ACR]: ACR[T] extends { reducer(s: S, action?: infer A): any }
    ? { prepare(...a: never[]): Omit<A, "type"> }
    : {};
};
interface Slice<
  State = any,
  CaseReducers extends SliceCaseReducers<State> = SliceCaseReducers<State>,
  Name extends string = string,
> {
  reducer: (state: State | undefined, action: UnknownAction) => State;
  name: Name;
  actions: CaseReducerActions<CaseReducers, Name>;
  getInitialState: () => State;
}
interface CreateSliceOptions<
  State = any,
  CR extends SliceCaseReducers<State> = SliceCaseReducers<State>,
  Name extends string = string,
> {
  name: Name;
  initialState: State | (() => State);
  reducers: ValidateSliceCaseReducers<State, CR>;
}
declare function createSlice<
  State,
  CaseReducers extends SliceCaseReducers<State>,
  Name extends string = string,
>(options: CreateSliceOptions<State, CaseReducers, Name>): Slice<State, CaseReducers, Name>;
