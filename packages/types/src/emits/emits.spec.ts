import { describe, it, assertType } from "vitest";
import { EmitsToProps, ComponentEmitsToProps } from "./emits";
import { UnionToIntersection } from "../helpers";

import { defineEmits, defineComponent } from "vue";

describe('"emits" helper', () => {
  describe("EmitsToProps", () => {
    describe("direct function types", () => {
      it("simple event", () => {
        type Fn = (e: "foo", a: number) => void;
        type Props = EmitsToProps<Fn>;
        type ExpectedProps = {
          onFoo: (a: number) => void;
        };

        assertType<Props>({} as ExpectedProps);

        // @ts-expect-error test for any type
        assertType<{ a: 1 }>({} as Props);

        const props = {} as Props;
        props.onFoo;
        props.onFoo?.(123);

        // @ts-expect-error optional
        props.onFoo(123);

        // @ts-expect-error wrong arg type
        props.onFoo?.("test");
        // @ts-expect-error non-existent prop
        props.onBar;
        // @ts-expect-error original event name not allowed
        props.foo;
      });

      it("no args event", () => {
        type Fn = (e: "foo") => void;
        type Props = EmitsToProps<Fn>;
        type ExpectedProps = {
          onFoo: () => void;
        };

        assertType<Props>({} as ExpectedProps);

        // @ts-expect-error test for any type
        assertType<{ a: 1 }>({} as Props);

        const props = {} as Props;
        props.onFoo;
        props.onFoo?.();
        // @ts-expect-error optional
        props.onFoo(123);
        // @ts-expect-error wrong number of args
        props.onFoo?.(123);
        // @ts-expect-error non-existent prop
        props.onBar;
      });

      it("multiple events with intersections", () => {
        type Fn = ((e: "foo", a: number) => void) &
          ((e: "bar", b: string) => void) &
          ((e: "baz") => void);

        type Props = EmitsToProps<Fn>;
        type ExpectedProps = {
          onFoo: (a: number) => void;
          onBar: (b: string) => void;
          onBaz: () => void;
        };

        assertType<Props>({} as ExpectedProps);

        // @ts-expect-error test for any type
        assertType<{ a: 1 }>({} as Props);

        const props = {} as Props;
        props.onFoo;
        props.onFoo?.(123);
        props.onBar;
        props.onBar?.("test");
        props.onBaz;
        props.onBaz?.();
        // @ts-expect-error optional
        props.onFoo(123);
        // @ts-expect-error wrong arg type
        props.onFoo?.("test");
        // @ts-expect-error wrong arg type
        props.onBar?.(123);
        // @ts-expect-error wrong number of args
        props.onBaz?.(123);
        // @ts-expect-error non-existent prop
        props.onQux;
      });

      it("multiple events with unions", () => {
        type Fn =
          | ((e: "foo", a: number) => void)
          | ((e: "bar", b: string) => void)
          | ((e: "baz") => void);

        type Props = UnionToIntersection<EmitsToProps<Fn>>;
        type ExpectedProps = {
          onFoo: (a: number) => void;
          onBar: (b: string) => void;
          onBaz: () => void;
        };

        assertType<Props>({} as ExpectedProps);

        // @ts-expect-error test for any type
        assertType<{ a: 1 }>({} as Props);

        const props = {} as Props;
        props.onFoo;
        props.onFoo?.(123);
        props.onBar;
        props.onBar?.("test");
        props.onBaz;
        props.onBaz?.();
        // @ts-expect-error optional
        props.onFoo(123);
        // @ts-expect-error wrong arg type
        props.onFoo?.("test");
        // @ts-expect-error wrong arg type
        props.onBar?.(123);
        // @ts-expect-error wrong number of args
        props.onBaz?.(123);
        // @ts-expect-error non-existent prop
        props.onQux;
      });

      it('event names "foo" | "bar"', () => {
        type Fn = (e: "foo" | "bar", a: number) => void;
        type Props = EmitsToProps<Fn>;
        type ExpectedProps = {
          onFoo: (a: number) => void;
          onBar: (a: number) => void;
        };

        assertType<Props>({} as ExpectedProps);

        // @ts-expect-error test for any type
        assertType<{ a: 1 }>({} as Props);

        const props = {} as Props;
        props.onFoo;
        props.onFoo?.(123);
        props.onBar;
        props.onBar?.(456);
        // @ts-expect-error optional
        props.onFoo(123);
        // @ts-expect-error wrong arg type
        props.onFoo?.("test");
        // @ts-expect-error wrong arg type
        props.onBar?.("test");
        // @ts-expect-error non-existent prop
        props.onBaz;
      });

      it("kebab-case event names", () => {
        type Fn = ((e: "update-value", value: string) => void) &
          ((e: "click-item", id: number) => void);

        type Props = EmitsToProps<Fn>;
        type ExpectedProps = {
          "onUpdate-value": (value: string) => void;
          "onClick-item": (id: number) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props["onUpdate-value"];
        props["onUpdate-value"]?.("test");
        props["onClick-item"];
        props["onClick-item"]?.(123);

        // @ts-expect-error optional
        props["onUpdate-value"]("test");
        // @ts-expect-error wrong arg type
        props["onUpdate-value"]?.(123);
        // @ts-expect-error wrong arg type
        props["onClick-item"]?.("test");
      });

      it("multiple arguments", () => {
        type Fn = (
          e: "change",
          value: string,
          index: number,
          meta: boolean
        ) => void;
        type Props = EmitsToProps<Fn>;
        type ExpectedProps = {
          onChange: (value: string, index: number, meta: boolean) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props.onChange;
        props.onChange?.("test", 123, true);
        // @ts-expect-error optional
        props.onChange("test", 123, true);
        // @ts-expect-error missing args
        props.onChange?.("test");
        // @ts-expect-error missing args
        props.onChange?.("test", 123);
        // @ts-expect-error wrong arg types
        props.onChange?.(123, "test", true);
      });
    });

    describe("defineEmits", () => {
      it("object syntax", () => {
        const emit = defineEmits({
          foo: null,
          bar: (a: number) => true,
          baz: (a: string, b: number) => true,
        });

        type Fn = typeof emit;
        type Props = EmitsToProps<Fn>;
        type ExpectedProps = {
          onFoo: (...args: any[]) => void;
          onBar: (a: number) => void;
          onBaz: (a: string, b: number) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props.onFoo;
        props.onFoo?.();
        props.onFoo?.(123);
        props.onBar;
        props.onBar?.(123);
        props.onBaz;
        props.onBaz?.("test", 456);
        // @ts-expect-error wrong arg type
        props.onBar?.("test");
        // @ts-expect-error missing args
        props.onBaz?.("test");
        // @ts-expect-error wrong arg type
        props.onBaz?.(123, 456);
      });

      it("array syntax", () => {
        const emit = defineEmits(["foo", "bar"]);

        type Fn = typeof emit;
        type Props = EmitsToProps<Fn>;
        type ExpectedProps = {
          onFoo: (...args: any[]) => void;
          onBar: (...args: any[]) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props.onFoo;
        props.onFoo?.();
        props.onFoo?.(123);
        props.onBar;
        props.onBar?.("test", 456);
        // @ts-expect-error optional
        props.onFoo();
        // @ts-expect-error non-existent prop
        props.onBaz;
      });

      describe("typed", () => {
        it("function syntax", () => {
          const emit = defineEmits<{
            foo: [a: number];
            bar: [b: string, c: boolean];
          }>();

          type Fn = typeof emit;
          type Props = EmitsToProps<Fn>;
          type ExpectedProps = {
            onFoo: (a: number) => void;
            onBar: (b: string, c: boolean) => void;
          };

          assertType<Props>({} as ExpectedProps);

          const props = {} as Props;
          props.onFoo;
          props.onFoo?.(123);
          props.onBar;
          props.onBar?.("test", true);
          // @ts-expect-error wrong arg type
          props.onFoo?.("test");
          // @ts-expect-error missing args
          props.onBar?.("test");
          // @ts-expect-error wrong arg type
          props.onBar?.(123, true);
        });

        it("object syntax", () => {
          const emit = defineEmits<{
            foo: [a: number];
            bar: [b: string, c: boolean];
            baz: [];
          }>();

          type Fn = typeof emit;
          type Props = EmitsToProps<Fn>;
          type ExpectedProps = {
            onFoo: (a: number) => void;
            onBar: (b: string, c: boolean) => void;
            onBaz: () => void;
          };

          assertType<Props>({} as ExpectedProps);

          const props = {} as Props;
          props.onFoo;
          props.onFoo?.(123);
          props.onBar;
          props.onBar?.("test", true);
          props.onBaz;
          props.onBaz?.();
          // @ts-expect-error wrong arg type
          props.onFoo?.("test");
          // @ts-expect-error missing args
          props.onBar?.("test");
          // @ts-expect-error wrong number of args
          props.onBaz?.(123);
        });
      });
    });

    describe("generic tests", () => {
      it("EmitsToProps preserves generic constraints", () => {
        function createEmitHandler<T extends string>(
          eventName: T
        ): (e: T, value: number) => void {
          return (e: T, value: number) => {};
        }

        function testEmitsToProps<T extends string>(eventName: T) {
          type EmitFn = (e: T, value: number) => void;
          type Props = EmitsToProps<EmitFn>;

          // The generic T should be preserved in the prop name
          const props = {} as Props;
          return props;
        }

        const result = testEmitsToProps("customEvent");
        type Result = typeof result;

        // Should have onCustomEvent property
        assertType<Result>(
          {} as {
            onCustomEvent: (value: number) => void;
          }
        );

        result.onCustomEvent;
        result.onCustomEvent?.(123);
        // @ts-expect-error not optional
        result.onCustomEvent(123);
        // @ts-expect-error wrong arg type
        result.onCustomEvent?.("test");
      });

      it("EmitsToProps with generic event types", () => {
        function createEmitProps<TEvent extends string, TValue>() {
          type EmitFn = (e: TEvent, value: TValue) => void;
          type Props = EmitsToProps<EmitFn>;
          return {} as Props;
        }

        const stringProps = createEmitProps<"update", string>();
        type StringProps = typeof stringProps;

        assertType<StringProps>(
          {} as {
            onUpdate: (value: string) => void;
          }
        );

        stringProps.onUpdate;
        stringProps.onUpdate?.("test");
        // @ts-expect-error not optional
        stringProps.onUpdate("test");
        // @ts-expect-error wrong arg type
        stringProps.onUpdate?.(123);

        const numberProps = createEmitProps<"change", number>();
        type NumberProps = typeof numberProps;

        assertType<NumberProps>(
          {} as {
            onChange: (value: number) => void;
          }
        );

        numberProps.onChange;
        numberProps.onChange?.(123);
        // @ts-expect-error not optional
        numberProps.onChange(123);
        // @ts-expect-error wrong arg type
        numberProps.onChange?.("test");
      });

      it("EmitsToProps with union of generic events", () => {
        function createMultiEmitProps<T extends "foo" | "bar", V>() {
          type EmitFn = (e: T, value: V) => void;
          type Props = EmitsToProps<EmitFn>;
          return {} as Props;
        }

        const props = createMultiEmitProps<"foo" | "bar", boolean>();
        type Props = typeof props;

        assertType<Props>(
          {} as {
            onFoo: (value: boolean) => void;
            onBar: (value: boolean) => void;
          }
        );

        props.onFoo;
        props.onFoo?.(true);
        props.onBar;
        props.onBar?.(false);
        // @ts-expect-error not optional
        props.onFoo(true);
        // @ts-expect-error wrong arg type
        props.onFoo?.("test");
        // @ts-expect-error wrong arg type
        props.onBar?.(123);
      });

      it("EmitsToProps with complex generic types", () => {
        function createTypedEmitProps<T extends Record<string, any>>() {
          // Create intersection type for proper event mapping
          type EmitFn = {
            [K in keyof T]: (e: K, value: T[K]) => void;
          }[keyof T] extends infer U
            ? U extends (e: infer E, value: infer V) => void
              ? (e: E, value: V) => void
              : never
            : never;

          // Need to create intersection manually for proper typing
          type EmitIntersection = UnionToIntersection<
            {
              [K in keyof T]: (e: K, value: T[K]) => void;
            }[keyof T]
          >;

          type Props = EmitsToProps<EmitIntersection>;
          return {} as Props;
        }

        interface MyEvents {
          update: string;
          change: number;
          submit: boolean;
        }

        const props = createTypedEmitProps<MyEvents>();
        type Props = typeof props;

        type ExpectedProps = {
          onUpdate: (value: string) => void;
          onChange: (value: number) => void;
          onSubmit: (value: boolean) => void;
        };

        assertType<Props>({} as ExpectedProps);

        props.onUpdate;
        props.onUpdate?.("test");
        props.onChange;
        props.onChange?.(123);
        props.onSubmit;
        props.onSubmit?.(true);
        // @ts-expect-error not optional
        props.onSubmit(true);
        // @ts-expect-error wrong arg type
        props.onUpdate?.(123);
        // @ts-expect-error wrong arg type
        props.onChange?.("test");
        // @ts-expect-error wrong arg type
        props.onSubmit?.("test");
      });

      it("EmitsToProps with generic intersection types", () => {
        function createIntersectionEmitProps<
          T1 extends string,
          T2 extends string,
          V1,
          V2
        >() {
          type EmitFn = ((e: T1, value: V1) => void) &
            ((e: T2, value: V2) => void);
          type Props = EmitsToProps<EmitFn>;
          return {} as Props;
        }

        const props = createIntersectionEmitProps<
          "input",
          "change",
          string,
          number
        >();
        type Props = typeof props;

        assertType<Props>(
          {} as {
            onInput: (value: string) => void;
            onChange: (value: number) => void;
          }
        );

        props.onInput;
        props.onInput?.("test");
        props.onChange;
        props.onChange?.(123);
        // @ts-expect-error not optional
        props.onInput("test");
        // @ts-expect-error wrong arg type
        props.onInput?.(123);
        // @ts-expect-error wrong arg type
        props.onChange?.("test");
      });
    });

    it("empty", () => {
      type Props = EmitsToProps<() => void>;
      assertType<Props>({} as {});

      // @ts-expect-error test for any type
      assertType<{ a: 1 }>({} as Props);
    });
  });

  describe("ComponentEmitsToProps", () => {
    describe("defineComponent with emits", () => {
      it("object syntax", () => {
        const component = defineComponent({
          emits: {
            foo: null,
            bar: (a: number) => true,
            baz: (a: string, b: number) => true,
          },
        });

        type Component = typeof component;
        type Props = ComponentEmitsToProps<Component>;
        type ExpectedProps = {
          onFoo: (...args: any[]) => void;
          onBar: (a: number) => void;
          onBaz: (a: string, b: number) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props.onFoo;
        props.onFoo?.();
        props.onFoo?.(123);
        props.onBar;
        props.onBar?.(123);
        props.onBaz;
        props.onBaz?.("test", 456);
        // @ts-expect-error wrong arg type
        props.onBar?.("test");
        // @ts-expect-error missing args
        props.onBaz?.("test");
        // @ts-expect-error wrong arg type
        props.onBaz?.(123, 456);
      });

      it("array syntax", () => {
        const component = defineComponent({
          emits: ["foo", "bar"],
        });

        type Component = typeof component;
        type Props = ComponentEmitsToProps<Component>;
        type ExpectedProps = {
          onFoo: (...args: any[]) => void;
          onBar: (...args: any[]) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props.onFoo;
        props.onFoo?.();
        props.onFoo?.(123);
        props.onBar;
        props.onBar?.("test", 456);
        // @ts-expect-error non-existent prop
        props.onBaz;
      });

      describe("typed", () => {
        it("with setup", () => {
          const component = defineComponent({
            emits: {
              foo: (a: number) => true,
              bar: (b: string, c: boolean) => true,
            },
            setup(props, { emit }) {
              emit("foo", 123);
              // @ts-expect-error
              emit("foo");
              emit("bar", "test", true);
              // @ts-expect-error
              emit("bar", "test");
            },
          });

          type Component = typeof component;
          type Props = ComponentEmitsToProps<Component>;
          type ExpectedProps = {
            onFoo: (a: number) => void;
            onBar: (b: string, c: boolean) => void;
          };

          assertType<Props>({} as ExpectedProps);

          const props = {} as Props;
          props.onFoo;
          props.onFoo?.(123);
          props.onBar;
          props.onBar?.("test", true);
          // @ts-expect-error wrong arg type
          props.onFoo?.("test");
          // @ts-expect-error missing args
          props.onBar?.("test");
          // @ts-expect-error wrong arg type
          props.onBar?.(123, true);
        });
      });

      it("kebab-case event names", () => {
        const component = defineComponent({
          emits: {
            "update-value": (value: string) => true,
            "click-item": (id: number) => true,
          },
        });

        type Component = typeof component;
        type Props = ComponentEmitsToProps<Component>;
        type ExpectedProps = {
          "onUpdate-value": (value: string) => void;
          "onClick-item": (id: number) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props["onUpdate-value"];
        props["onUpdate-value"]?.("test");
        props["onClick-item"];
        props["onClick-item"]?.(123);
        // @ts-expect-error wrong arg type
        props["onUpdate-value"]?.(123);
        // @ts-expect-error wrong arg type
        props["onClick-item"]?.("test");
      });

      it("multiple arguments", () => {
        const component = defineComponent({
          emits: {
            change: (value: string, index: number, meta: boolean) => true,
          },
        });

        type Component = typeof component;
        type Props = ComponentEmitsToProps<Component>;
        type ExpectedProps = {
          onChange: (value: string, index: number, meta: boolean) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props.onChange;
        props.onChange?.("test", 123, true);
        // @ts-expect-error missing args
        props.onChange?.("test");
        // @ts-expect-error missing args
        props.onChange?.("test", 123);
        // @ts-expect-error wrong arg types
        props.onChange?.(123, "test", true);
      });

      it("no args event", () => {
        const component = defineComponent({
          emits: {
            click: () => true,
            submit: () => true,
          },
        });

        type Component = typeof component;
        type Props = ComponentEmitsToProps<Component>;
        type ExpectedProps = {
          onClick: () => void;
          onSubmit: () => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props.onClick;
        props.onClick?.();
        props.onSubmit;
        props.onSubmit?.();
        // @ts-expect-error not optional
        props.onClick();
        // @ts-expect-error wrong number of args
        props.onClick?.(123);
        // @ts-expect-error wrong number of args
        props.onSubmit?.("test");
      });

      it("component with props and emits", () => {
        const component = defineComponent({
          props: {
            value: String,
            count: Number,
          },
          emits: {
            "update:value": (value: string) => true,
            "update:count": (count: number) => true,
          },
        });

        type Component = typeof component;
        type Props = ComponentEmitsToProps<Component>;
        type ExpectedProps = {
          "onUpdate:value": (value: string) => void;
          "onUpdate:count": (count: number) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props["onUpdate:value"];
        props["onUpdate:value"]?.("test");
        props["onUpdate:count"];
        props["onUpdate:count"]?.(123);
        // @ts-expect-error not optional
        props["onUpdate:value"]("test");
        // @ts-expect-error not optional
        props["onUpdate:count"](123);
        // @ts-expect-error wrong arg type
        props["onUpdate:value"]?.(123);
        // @ts-expect-error wrong arg type
        props["onUpdate:count"]?.("test");
      });
    });

    describe("generic tests", () => {
      it("ComponentEmitsToProps preserves generic event types", () => {
        function createGenericComponent<T extends string, V>(
          eventName: T,
          validator: (value: V) => boolean
        ) {
          return defineComponent({
            emits: {
              [eventName]: validator,
            } as Record<T, (value: V) => boolean>,
          });
        }

        function testComponentProps<T extends string, V>(
          eventName: T,
          validator: (value: V) => boolean
        ) {
          const component = createGenericComponent(eventName, validator);
          type Component = typeof component;
          type Props = ComponentEmitsToProps<Component>;
          return {} as any as Props;
        }

        const stringProps = testComponentProps(
          "update",
          (value: string) => true
        );

        stringProps.onUpdate;
        stringProps.onUpdate?.("test");
        // @ts-expect-error wrong arg type
        stringProps.onUpdate?.(123);

        const numberProps = testComponentProps(
          "change",
          (value: number) => true
        );

        numberProps.onChange;
        numberProps.onChange?.(123);
        // @ts-expect-error wrong arg type
        numberProps.onChange?.("test");
      });

      it("ComponentEmitsToProps with generic payload types", () => {
        function createTypedComponent<TPayload>() {
          return defineComponent({
            emits: {
              update: (payload: TPayload) => true,
              change: (payload: TPayload) => true,
            },
          });
        }

        interface MyPayload {
          id: number;
          name: string;
        }

        const component = createTypedComponent<MyPayload>();
        type Component = typeof component;
        type Props = ComponentEmitsToProps<Component>;

        type ExpectedProps = {
          onUpdate: (payload: MyPayload) => void;
          onChange: (payload: MyPayload) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props.onUpdate;
        props.onUpdate?.({ id: 1, name: "test" });
        props.onChange;
        props.onChange?.({ id: 2, name: "test2" });
        // @ts-expect-error wrong arg type
        props.onUpdate?.("test");
        // @ts-expect-error wrong arg type
        props.onChange?.(123);
        // @ts-expect-error missing properties
        props.onUpdate?.({ id: 1 });
      });

      it("ComponentEmitsToProps with multiple generic types", () => {
        function createMultiGenericComponent<T1, T2, T3>() {
          return defineComponent({
            emits: {
              first: (value: T1) => true,
              second: (value: T2) => true,
              third: (value: T3) => true,
            },
          });
        }

        const component = createMultiGenericComponent<
          string,
          number,
          boolean
        >();
        type Component = typeof component;
        type Props = ComponentEmitsToProps<Component>;

        type ExpectedProps = {
          onFirst: (value: string) => void;
          onSecond: (value: number) => void;
          onThird: (value: boolean) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props.onFirst;
        props.onFirst?.("test");
        props.onSecond;
        props.onSecond?.(123);
        props.onThird;
        props.onThird?.(true);
        // @ts-expect-error not optional
        props.onFirst("test");
        // @ts-expect-error not optional
        props.onSecond(123);
        // @ts-expect-error not optional
        props.onThird(true);
        // @ts-expect-error wrong arg type
        props.onFirst?.(123);
        // @ts-expect-error wrong arg type
        props.onSecond?.("test");
        // @ts-expect-error wrong arg type
        props.onThird?.("test");
      });

      it("ComponentEmitsToProps with constrained generics", () => {
        function createConstrainedComponent<
          T extends { id: number; name: string }
        >() {
          return defineComponent({
            emits: {
              update: (item: T) => true,
              delete: (id: T["id"]) => true,
            },
          });
        }

        interface User {
          id: number;
          name: string;
          email: string;
        }

        const component = createConstrainedComponent<User>();
        type Component = typeof component;
        type Props = ComponentEmitsToProps<Component>;

        type ExpectedProps = {
          onUpdate: (item: User) => void;
          onDelete: (id: number) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props.onUpdate;
        props.onUpdate?.({ id: 1, name: "John", email: "john@example.com" });
        props.onDelete;
        props.onDelete?.(1);
        // @ts-expect-error not optional
        props.onDelete(1);
        // @ts-expect-error missing properties
        props.onUpdate?.({ id: 1, name: "John" });
        // @ts-expect-error wrong arg type
        props.onDelete?.("test");
      });

      it("ComponentEmitsToProps with array generic types", () => {
        function createArrayComponent<T>() {
          return defineComponent({
            emits: {
              update: (items: T[]) => true,
              add: (item: T) => true,
              remove: (item: T) => true,
            },
          });
        }

        const component = createArrayComponent<string>();
        type Component = typeof component;
        type Props = ComponentEmitsToProps<Component>;

        type ExpectedProps = {
          onUpdate: (items: string[]) => void;
          onAdd: (item: string) => void;
          onRemove: (item: string) => void;
        };

        assertType<Props>({} as ExpectedProps);

        const props = {} as Props;
        props.onUpdate;
        props.onUpdate?.(["a", "b", "c"]);
        props.onAdd;
        props.onAdd?.("test");
        props.onRemove;
        props.onRemove?.("test");
        // @ts-expect-error not optional
        props.onAdd("test");
        // @ts-expect-error not optional
        props.onRemove("test");
        // @ts-expect-error wrong arg type
        props.onUpdate?.([1, 2, 3]);
        // @ts-expect-error wrong arg type
        props.onAdd?.(123);
        // @ts-expect-error wrong arg type
        props.onRemove?.(123);
      });
    });
  });
});
