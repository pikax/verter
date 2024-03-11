import {
  DeclareComponent,
  ComponentProps,
  ComponentEmits,
  ComponentData,
  ComponentSlots,
  ModelRef,
  SlotsType,
  ReservedProps,
  Component,
  IntrinsicElementAttributes,
  Ref,
  GlobalComponents,
} from "vue";

declare module "vue" {
  interface GlobalComponents {
    GlobalSupa: { new (): { $props: { foo: string } } };
  }
}
declare global {
  var __COMP__: DeclareComponent;

  function expectType<T>(value: T): void;

  // extract

  function getComponentProps<T>(component: T): ComponentProps<T>;
  function getComponentEmits<T>(component: T): ComponentEmits<T>;
  function getComponentData<T>(component: T): ComponentData<T>;
  function getComponentSlots<T>(component: T): ComponentSlots<T>;

  type ExtractModelType<T> = T extends ModelRef<infer V> ? V : any;

  /**
   * Converts slots type to component to be used in JSX
   */
  type SlotsToComponent<S> = S extends SlotsType<infer X>
    ? {
        new <N extends keyof X>(): {
          $props: {
            name: N;
          } & (Parameters<X[N]>[0] extends infer P & {} ? P : {}) & {
              [K in keyof ReservedProps]?: never;
            };
        };
      }
    : never;

  type OmitNever<T> = {
    [K in keyof T as T[K] extends never ? never : K]: T[K];
  };

  type IsComponent<T> = T extends Ref<infer V>
    ? IsComponent<V>
    : T extends Component
    ? T
    : never;

  type IsHTMLComponent<T> = T extends Ref<infer V>
    ? IsHTMLComponent<V>
    : T extends keyof IntrinsicElementAttributes
    ? T
    : never;

  type ExtractComponent<T> = T extends Ref<infer V>
    ? ExtractComponent<V>
    : T extends Component
    ? T
    : T extends keyof IntrinsicElementAttributes
    ? { new (): { $props: IntrinsicElementAttributes[T] } }
    : never;

  type ExtractRenderComponents<T> = OmitNever<{
    [K in keyof T as K extends `$${string}` ? never : K]: ExtractComponent<
      T[K]
    >;
  }> &
    GlobalComponents & {};
}
