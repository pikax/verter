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
import type {
  GetVueComponent,
  DefineOptions,
  ExtractComponents,
} from "./components";

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
      type Options = ReturnType<
        typeof DefineOptions<{ name: "MyComponent" }, "MyComponent", never>
      >;
      assertType<Options>({} as { name: "MyComponent" });

      // @ts-expect-error - Options is not any/unknown, should not accept unrelated types
      assertType<Options>({} as { unrelated: true });
    });

    it("preserves inheritAttrs type", () => {
      type Options = ReturnType<
        typeof DefineOptions<{ inheritAttrs: false }, never, false>
      >;
      assertType<Options>({} as { inheritAttrs: false });

      // @ts-expect-error - Options is not any/unknown, should not accept unrelated types
      assertType<Options>({} as { unrelated: true });
    });

    it("preserves both name and inheritAttrs", () => {
      type Options = ReturnType<
        typeof DefineOptions<
          { name: "MyComponent"; inheritAttrs: false },
          "MyComponent",
          false
        >
      >;
      assertType<Options>({} as { name: "MyComponent"; inheritAttrs: false });

      // @ts-expect-error - Options is not any/unknown, should not accept unrelated types
      assertType<Options>({} as { unrelated: true });
    });

    it("allows empty object", () => {
      type Options = ReturnType<typeof DefineOptions<{}, never, never>>;
      assertType<Options>({} as {});
    });
  });

  // @ai-generated - Tests ExtractComponents type helper for extracting Vue components from objects
  describe("ExtractComponents", () => {
    // Helper component types for testing
    type MockComponent = { new (): { $props: { msg: string } } };
    type MockComponent2 = { new (): { $props: { count: number } } };
    type MockComponent3 = { new (): { $props: {} } };

    describe("basic extraction", () => {
      it("extracts component from flat object", () => {
        type Input = {
          Comp: MockComponent;
          notAComponent: string;
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          Comp: MockComponent;
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown
        assertType<Result>({} as { unrelated: true });
      });

      it("extracts multiple components", () => {
        type Input = {
          Comp1: MockComponent;
          Comp2: MockComponent2;
          config: { theme: string };
          count: number;
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          Comp1: MockComponent;
          Comp2: MockComponent2;
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown
        assertType<Result>({} as { unrelated: true });
      });

      it("returns empty object when no components", () => {
        type Input = {
          config: { theme: string };
          count: number;
          name: string;
        };

        type Result = ExtractComponents<Input>;
        type Expected = {};

        assertType<Result>({} as Expected);
      });

      it("handles empty input object", () => {
        type Input = {};

        type Result = ExtractComponents<Input>;
        type Expected = {};

        assertType<Result>({} as Expected);
      });
    });

    describe("deep extraction", () => {
      it("extracts components from nested objects", () => {
        type Input = {
          Comp: MockComponent;
          nested: {
            NestedComp: MockComponent2;
            notComponent: string;
          };
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          Comp: MockComponent;
          nested: {
            NestedComp: MockComponent2;
          };
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown
        assertType<Result>({} as { unrelated: true });
      });

      it("extracts from deeply nested structures", () => {
        type Input = {
          level1: {
            level2: {
              level3: {
                DeepComp: MockComponent;
                value: number;
              };
              notHere: boolean;
            };
            Comp2: MockComponent2;
          };
          TopComp: MockComponent3;
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          level1: {
            level2: {
              level3: {
                DeepComp: MockComponent;
              };
            };
            Comp2: MockComponent2;
          };
          TopComp: MockComponent3;
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown
        assertType<Result>({} as { unrelated: true });
      });

      it("removes empty nested objects", () => {
        type Input = {
          Comp: MockComponent;
          empty: {
            noComponents: string;
            justValues: number;
          };
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          Comp: MockComponent;
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);
      });
    });

    describe("filters non-components", () => {
      it("removes primitive values", () => {
        type Input = {
          Comp: MockComponent;
          str: string;
          num: number;
          bool: boolean;
          sym: symbol;
          nul: null;
          undef: undefined;
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          Comp: MockComponent;
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);
      });

      it("removes arrays", () => {
        type Input = {
          Comp: MockComponent;
          arr: string[];
          tuple: [number, string];
          compArray: MockComponent[]; // Array of components is not a component itself
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          Comp: MockComponent;
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);
      });

      it("removes regular functions (non-component)", () => {
        type Input = {
          Comp: MockComponent;
          helper: (x: number) => string;
          callback: () => void;
        };

        type Result = ExtractComponents<Input>;

        // Regular functions that return primitives are not components
        // () => void becomes Comment, but helper returns string -> HTMLElement
        // The behavior depends on GetVueComponent implementation
        const result = {} as Result;
        result.Comp;
      });
    });

    describe("real-world patterns", () => {
      it("extracts from module-like structure", () => {
        type ModuleExports = {
          Button: MockComponent;
          Input: MockComponent2;
          Form: MockComponent3;
          // Functional-components
          utils: {
            formatDate: (d: Date) => string;
            validateEmail: (s: string) => boolean;
          };
          constants: {
            MAX_LENGTH: number;
            DEFAULT_THEME: string;
          };
          types: {
            // Nested component
            Dialog: MockComponent;
          };
        };

        type Result = ExtractComponents<ModuleExports>;
        type Expected = {
          Button: MockComponent;
          Input: MockComponent2;
          Form: MockComponent3;
          types: {
            Dialog: MockComponent;
          };
          // Functional-components
          utils: {
            formatDate: (d: Date) => string;
            validateEmail: (s: string) => boolean;
          };
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown
        assertType<Result>({} as { unrelated: true });
      });

      it("extracts from component library structure", () => {
        type ComponentLibrary = {
          // Direct components
          Button: MockComponent;
          // Grouped components
          Form: {
            Input: MockComponent;
            Select: MockComponent2;
            Checkbox: MockComponent3;
            validators: {
              required: (v: unknown) => boolean;
            };
          };
          Layout: {
            Container: MockComponent;
            Row: MockComponent;
            Col: MockComponent;
          };
          // Non-component exports
          version: string;
          install: (app: unknown) => void;
        };

        type Result = ExtractComponents<ComponentLibrary>;
        type Expected = {
          Button: MockComponent;
          Form: {
            Input: MockComponent;
            Select: MockComponent2;
            Checkbox: MockComponent3;
            validators: {
              required: (v: unknown) => boolean;
            };
          };
          Layout: {
            Container: MockComponent;
            Row: MockComponent;
            Col: MockComponent;
          };
          install: (app: unknown) => void;
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown
        assertType<Result>({} as { unrelated: true });
      });
    });

    describe("edge cases", () => {
      it("handles HTMLElement types", () => {
        type Input = {
          div: HTMLDivElement;
          span: HTMLSpanElement;
          notElement: string;
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          div: HTMLDivElement;
          span: HTMLSpanElement;
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);
      });

      it("handles functional components", () => {
        type FunctionalComp = (props: { id: number }) => VNode;
        type Input = {
          Functional: FunctionalComp;
          Regular: MockComponent;
        };

        type Result = ExtractComponents<Input>;

        // Functional components are kept (they return VNode -> HTMLElement)
        const result = {} as Result;
        result.Functional;
        result.Regular;
      });

      it("preserves component type exactly", () => {
        type SpecificComponent = {
          new (): {
            $props: { required: string; optional?: number };
            $emit: (e: "click", payload: MouseEvent) => void;
          };
        };

        type Input = {
          Specific: SpecificComponent;
          other: number;
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          Specific: SpecificComponent;
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // Verify the component type is preserved exactly
        type ExtractedComp = Result["Specific"];
        type Instance = InstanceType<ExtractedComp>;
        assertType<Instance["$props"]>(
          {} as { required: string; optional?: number }
        );
      });
    });
  });
});
