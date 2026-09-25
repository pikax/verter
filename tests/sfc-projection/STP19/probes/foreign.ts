// Third-party Vue component dependencies the STP19 probes consume, written
// against the real `vue` declarations: an Options API constructor (Vue
// publishes it with open `...args: any[]`), a generic setup-function
// constructor, a functional component, a generic functional component, a
// hand-written constructor with six construct overloads, a component whose
// publisher erased its generic, an untyped (`any`) component, a hand-written
// constructor whose precise overloads precede an open `...args: any[]`
// catch-all, and a functional component with three call overloads.
import {
  defineComponent,
  h,
  type FunctionalComponent,
  type SetupContext,
  type SlotsType,
  type VNode,
} from "vue";

export const Counter = defineComponent({
  props: { count: { type: Number, required: true } },
  emits: { bump: (by: number) => by > 0 },
  setup() {
    return () => h("span");
  },
});

export const Choice = defineComponent(
  <T extends string | number>(
    props: { value: T; options: T[] },
    ctx: SetupContext<{ change: (value: T) => true }>,
  ) =>
    () =>
      h("select", { onChange: () => ctx.emit("change", props.value) }),
  { props: ["value", "options"], emits: ["change"] },
);

type Level = 1 | 2 | 3;

export const Badge: FunctionalComponent<
  { level: Level },
  { dismiss: (level: Level) => true },
  { default: (props: { level: Level }) => VNode[] }
> = (props, { slots }) => h("span", slots.default?.(props));

export function Cell<T>(
  props: { value: T; render: (value: T) => string },
  ctx: SetupContext<{ edit: (value: T) => true }, SlotsType<{ default: (props: { value: T }) => VNode[] }>>,
): VNode {
  return h("td", { onClick: () => ctx.emit("edit", props.value) }, props.render(props.value));
}

interface ShapeInstance<P, K extends string> {
  readonly $props: P;
  readonly kind: K;
}

export declare const Shape: {
  new (props: { kind: "circle"; radius: number }): ShapeInstance<{ kind: "circle"; radius: number }, "circle">;
  new (props: { kind: "square"; side: number }): ShapeInstance<{ kind: "square"; side: number }, "square">;
  new (props: { kind: "rect"; width: number; height: number }): ShapeInstance<{ kind: "rect"; width: number; height: number }, "rect">;
  new (props: { kind: "line"; length: number }): ShapeInstance<{ kind: "line"; length: number }, "line">;
  new (props: { kind: "polygon"; sides: number }): ShapeInstance<{ kind: "polygon"; sides: number }, "polygon">;
  new (props: { kind: "point" }): ShapeInstance<{ kind: "point" }, "point">;
};

export declare const ErasedList: new (props: { items: readonly unknown[] }) => {
  readonly $props: { items: readonly unknown[] };
  readonly $slots: { default(props: { item: unknown }): VNode[] };
};

export declare const Untyped: any;

export declare const Gauge: {
  new (props: { unit: "celsius"; value: number }): { readonly $props: { unit: "celsius"; value: number }; readonly unit: "celsius" };
  new (props: { unit: "percent"; ratio: number }): { readonly $props: { unit: "percent"; ratio: number }; readonly unit: "percent" };
  new (...args: any[]): { readonly $props: { unit: "raw"; raw: string }; readonly unit: "raw" };
};

export function Toggle(props: { mode: "on"; level: number }, ctx: SetupContext<{ flip: (level: number) => true }>): VNode;
export function Toggle(props: { mode: "off"; reason: string }, ctx: SetupContext<{ reset: () => true }>): VNode;
export function Toggle(props: { mode: "auto"; enabled: boolean }): VNode;
export function Toggle(props: { mode: string }): VNode {
  return h("button", props.mode);
}
