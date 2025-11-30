import { ExtractHidden, IntersectionFunctionToObject } from "../helpers";

/**
 * Converts event emission function types into Vue props types.
 * For each event name `K`, it creates a prop named `onK` (capitalized)
 * that is a function accepting the same arguments as the event.
 */
export type EmitsToProps<T> = T extends () => any
  ? {}
  : IntersectionFunctionToObject<T> extends infer O
  ? ExtractHidden<O> extends infer E extends Record<PropertyKey, any>
    ? {
        [K in keyof E as `on${Capitalize<K & string>}`]?: (
          ...args: E[K]
        ) => void;
      }
    : {}
  : {};

/**
 * Extracts emit event types from a Vue component and converts them to props.
 * Works with components created via defineComponent.
 */
export type ComponentEmitsToProps<T> = T extends new (
  ...args: any[]
) => infer Instance
  ? Instance extends { $emit: infer EmitFn }
    ? EmitsToProps<EmitFn>
    : {}
  : {};

export declare function eventCallbacks<
  TArgs extends Array<any>,
  R extends ($event: TArgs[0]) => any
>(event: TArgs, cb: R): R;

// function onFoo(e: number, b: string): void {}

// declare function makeCallbacks<T extends (...args: any[]) => void>(
//   o: T
// ): undefined | ((cb: T) => void);

// makeCallbacks(onFoo)((...args) => {
//   eventCallbacks(args, (e) => {});
// });
