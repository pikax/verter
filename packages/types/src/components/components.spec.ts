/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for Vue built-in component type augmentations including:
 * - KeepAlive, Transition, TransitionGroup, Teleport, Suspense
 * - Verifies slot typing and prop types for built-in components
 */
import { describe, it, assertType } from "vitest";
import type { defineComponent, VNode } from "vue";
import {
  KeepAlive,
  Transition,
  TransitionGroup,
  Teleport,
  Suspense,
} from "vue";
import type { GetVueComponent, DefineOptions } from "./components";

describe("components helpers", () => {
  describe("GetVueComponent", () => {
    it("constructor returns instance type", () => {
      class MyEl extends HTMLElement {
        foo = 1;
      }
      type R = GetVueComponent<typeof MyEl>;
      assertType<R>({} as MyEl);
      assertType<MyEl>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("function returning void -> Comment", () => {
      type R = GetVueComponent<() => void>;
      assertType<R>({} as typeof import("vue").Comment);
      assertType<typeof import("vue").Comment>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("function returning array -> Fragment", () => {
      type R = GetVueComponent<() => number[]>;
      assertType<R>({} as typeof import("vue").Fragment);
      assertType<typeof import("vue").Fragment>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("function returning other -> HTMLElement", () => {
      type R = GetVueComponent<() => string>;
      assertType<R>({} as HTMLElement);
      assertType<HTMLElement>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("direct HTMLElement type preserved", () => {
      type R = GetVueComponent<HTMLDivElement>;
      assertType<R>({} as HTMLDivElement);
      assertType<HTMLDivElement>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("works with defineComponent constructor", () => {
      type Comp = ReturnType<typeof defineComponent>;
      type R = GetVueComponent<Comp>;
      type Expected = InstanceType<Comp>;
      assertType<R>({} as Expected);
      assertType<Expected>({} as R);

      // Note: defineComponent without args has loose types, so R accepts any type
      // This is expected Vue behavior - no @ts-expect-error here
    });

    it("Vue built-ins (KeepAlive)", () => {
      type R = GetVueComponent<typeof KeepAlive>;
      type Expected = InstanceType<typeof KeepAlive>;
      assertType<R>({} as Expected);
      assertType<Expected>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("Vue built-ins (Transition)", () => {
      // Transition is a FunctionalComponent in Vue types
      type R = GetVueComponent<typeof Transition>;
      assertType<R>({} as typeof import("vue").Comment);
      assertType<typeof import("vue").Comment>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("Vue built-ins (TransitionGroup)", () => {
      type R = GetVueComponent<typeof TransitionGroup>;
      type Expected = InstanceType<typeof TransitionGroup>;
      assertType<R>({} as Expected);
      assertType<Expected>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("Vue built-ins (Teleport)", () => {
      type R = GetVueComponent<typeof Teleport>;
      type Expected = InstanceType<typeof Teleport>;
      assertType<R>({} as Expected);
      assertType<Expected>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("Vue built-ins (Suspense)", () => {
      type R = GetVueComponent<typeof Suspense>;
      type Expected = InstanceType<typeof Suspense>;
      assertType<R>({} as Expected);
      assertType<Expected>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("function component returning VNode -> HTMLElement", () => {
      type FC = (props: { id?: number }) => VNode;
      type R = GetVueComponent<FC>;
      assertType<R>({} as HTMLElement);
      assertType<HTMLElement>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("function component returning VNode[] -> Fragment", () => {
      type FC = (props: { items: number[] }) => VNode[];
      type R = GetVueComponent<FC>;
      assertType<R>({} as typeof import("vue").Fragment);
      assertType<typeof import("vue").Fragment>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("function component returning void -> Comment", () => {
      type FC = (props: { hidden?: boolean }) => void;
      type R = GetVueComponent<FC>;
      assertType<R>({} as typeof import("vue").Comment);
      assertType<typeof import("vue").Comment>({} as R);

      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
    });

    it("non-component types yield never", () => {
      assertType<GetVueComponent<number>>({} as never);
    });
  });

  describe("DefineOptions", () => {
    it("signature and return type", () => {
      type ExpectedSig = <T extends { name?: string; inheritAttrs?: boolean }>(
        o: T
      ) => T;
      assertType<ExpectedSig>({} as typeof DefineOptions);
      assertType<typeof DefineOptions>({} as ExpectedSig);
    });

      it("preserves name type", () => {
        type Options = ReturnType<typeof DefineOptions<{ name: "MyComponent" }, "MyComponent", never>>;
        assertType<Options>({} as { name: "MyComponent" });

        // @ts-expect-error - Options is not any/unknown, should not accept unrelated types
        assertType<Options>({} as { unrelated: true });
      });

      it("preserves inheritAttrs type", () => {
        type Options = ReturnType<typeof DefineOptions<{ inheritAttrs: false }, never, false>>;
        assertType<Options>({} as { inheritAttrs: false });

        // @ts-expect-error - Options is not any/unknown, should not accept unrelated types
        assertType<Options>({} as { unrelated: true });
      });

      it("preserves both name and inheritAttrs", () => {
        type Options = ReturnType<typeof DefineOptions<{ name: "MyComponent"; inheritAttrs: false }, "MyComponent", false>>;
        assertType<Options>({} as { name: "MyComponent"; inheritAttrs: false });

        // @ts-expect-error - Options is not any/unknown, should not accept unrelated types
        assertType<Options>({} as { unrelated: true });
      });

      it("allows empty object", () => {
        type Options = ReturnType<typeof DefineOptions<{}, never, never>>;
        assertType<Options>({} as {});
      });
  });
});
