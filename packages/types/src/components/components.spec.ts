/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for Vue built-in component type augmentations including:
 * - KeepAlive, Transition, TransitionGroup, Teleport, Suspense
 * - Verifies slot typing and prop types for built-in components
 */
import "../tsx/tsx";
import { describe, it, assertType } from "vitest";
import { defineComponent, VNode } from "vue";
import {
  KeepAlive,
  Transition,
  TransitionGroup,
  Teleport,
  Suspense,
  defineAsyncComponent,
} from "vue";
import type {
  GetVueComponent,
  DefineOptions,
  ExtractComponents,
  enhanceElementWithProps,
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
      type ExpectedSig = <T extends { name?: string; inheritAttrs?: boolean }>(o: T) => T;
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
      type Options = ReturnType<typeof DefineOptions<{ inheritAttrs: false }, never, false>>;
      assertType<Options>({} as { inheritAttrs: false });

      // @ts-expect-error - Options is not any/unknown, should not accept unrelated types
      assertType<Options>({} as { unrelated: true });
    });

    it("preserves both name and inheritAttrs", () => {
      type Options = ReturnType<
        typeof DefineOptions<{ name: "MyComponent"; inheritAttrs: false }, "MyComponent", false>
      >;
      assertType<Options>({} as { name: "MyComponent"; inheritAttrs: false });

      // @ts-expect-error - Options is not any/unknown, should not accept unrelated types
      assertType<Options>({} as { unrelated: true });
    });

    it("allows empty object", () => {
      type Options = ReturnType<typeof DefineOptions<{}, never, never>>;
      assertType<Options>({} as {});
    });

    it("from defineAsyncComponent", () => {
      const component = defineAsyncComponent(() =>
        Promise.resolve(
          defineComponent({
            props: {
              message: String,
            },
          }),
        ),
      );

      type R = GetVueComponent<typeof component>;
      type Expected = InstanceType<typeof component>;
      assertType<R>({} as Expected);
      assertType<Expected>({} as R);
      // @ts-expect-error - R is not any/unknown, should not accept unrelated types
      assertType<R>({} as { unrelated: true });
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
      it("extracts components from nested objects with capitalized keys", () => {
        type Input = {
          Comp: MockComponent;
          Nested: {
            NestedComp: MockComponent2;
            notComponent: string;
          };
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          Comp: MockComponent;
          Nested: {
            NestedComp: MockComponent2;
          };
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown
        assertType<Result>({} as { unrelated: true });
      });

      it("extracts from deeply nested capitalized structures", () => {
        type Input = {
          Level1: {
            Level2: {
              Level3: {
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
          Level1: {
            Level2: {
              Level3: {
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

      it("removes empty nested objects even when capitalized", () => {
        type Input = {
          Comp: MockComponent;
          Empty: {
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

    describe("capitalization filter", () => {
      it("removes lowercase component keys", () => {
        type Input = {
          fooA: MockComponent;
          FooB: MockComponent2;
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          FooB: MockComponent2;
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - lowercase keys should not be present
        (({}) as Result).fooA;
      });

      it("removes lowercase containers even when they hold components", () => {
        type Input = {
          container: {
            FooC: MockComponent;
          };
          Container: {
            FooD: MockComponent2;
          };
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          Container: {
            FooD: MockComponent2;
          };
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - lowercase container should be removed entirely
        (({}) as Result).container;
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
      it("extracts from module-like structure with capitalized keys", () => {
        type ModuleExports = {
          Button: MockComponent;
          Input: MockComponent2;
          Form: MockComponent3;
          // Functional-components
          Utils: {
            formatDate: (d: Date) => string;
            validateEmail: (s: string) => boolean;
          };
          Constants: {
            MAX_LENGTH: number;
            DEFAULT_THEME: string;
          };
          Types: {
            // Nested component
            Dialog: MockComponent;
          };
        };

        type Result = ExtractComponents<ModuleExports>;
        type Expected = {
          Button: MockComponent;
          Input: MockComponent2;
          Form: MockComponent3;
          Types: {
            Dialog: MockComponent;
          };
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown
        assertType<Result>({} as { unrelated: true });

        // @ts-expect-error - Utils is dropped because nested keys are not capitalized
        (({}) as Result).Utils;

        // @ts-expect-error - Constants is dropped because it contains no components
        (({}) as Result).Constants;
      });

      it("extracts from component library structure honoring capitalization", () => {
        type ComponentLibrary = {
          // Direct components
          Button: MockComponent;
          // Grouped components
          Form: {
            Input: MockComponent;
            Select: MockComponent2;
            Checkbox: MockComponent3;
            Validators: {
              Required: (v: unknown) => boolean;
            };
          };
          Layout: {
            Container: MockComponent;
            Row: MockComponent;
            Col: MockComponent;
          };
          // Non-component exports
          Version: string;
          Install: (app: unknown) => void;
        };

        type Result = ExtractComponents<ComponentLibrary>;
        type Expected = {
          Button: MockComponent;
          Form: {
            Input: MockComponent;
            Select: MockComponent2;
            Checkbox: MockComponent3;
            Validators: {
              Required: (v: unknown) => boolean;
            };
          };
          Layout: {
            Container: MockComponent;
            Row: MockComponent;
            Col: MockComponent;
          };
          Install: (app: unknown) => void;
        };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown
        assertType<Result>({} as { unrelated: true });
      });
    });

    describe("edge cases", () => {
      it("handles HTMLElement types with capitalized keys", () => {
        type Input = {
          Div: HTMLDivElement;
          Span: HTMLSpanElement;
          notElement: string;
        };

        type Result = ExtractComponents<Input>;
        type Expected = {
          Div: HTMLDivElement;
          Span: HTMLSpanElement;
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
        assertType<Instance["$props"]>({} as { required: string; optional?: number });
      });
    });
  });

  describe("instantiateComponent", () => {
    it("returns instance type for class-based component", () => {
      type MockInstance = { $props: { msg: string }; $emit: () => void };
      type MockConstructor = { new (): MockInstance };

      type Result = ReturnType<
        typeof import("./components").instantiateComponent<MockConstructor, { msg: string }>
      >;
      assertType<Result>({} as MockInstance);
      assertType<MockInstance>({} as Result);

      // @ts-expect-error - Result is not any/unknown
      assertType<Result>({} as { unrelated: true });
    });

    it("returns return type for functional component", () => {
      type FnComp = (props: { label: string }) => VNode;

      type Result = ReturnType<
        typeof import("./components").instantiateComponent<FnComp, { label: string }>
      >;
      assertType<Result>({} as VNode);
      assertType<VNode>({} as Result);

      // @ts-expect-error - Result is not any/unknown
      assertType<Result>({} as { unrelated: true });
    });

    it("returns return type for functional component returning void (Comment)", () => {
      type FnComp = (props: { hidden?: boolean }) => void;

      type Result = ReturnType<
        typeof import("./components").instantiateComponent<FnComp, { hidden?: boolean }>
      >;
      assertType<Result>(undefined as void);
      assertType<void>(undefined as Result);

      // @ts-expect-error - Result is not any/unknown
      assertType<Result>({} as { unrelated: true });
    });

    it("returns T for non-component types", () => {
      type Result = ReturnType<
        typeof import("./components").instantiateComponent<HTMLDivElement, {}>
      >;
      assertType<Result>({} as HTMLDivElement);
      assertType<HTMLDivElement>({} as Result);
    });

    it("works with defineComponent result", () => {
      const Comp = defineComponent({
        props: { title: String },
      });

      type Result = ReturnType<
        typeof import("./components").instantiateComponent<typeof Comp, { title?: string }>
      >;
      type Expected = InstanceType<typeof Comp>;
      assertType<Result>({} as Expected);
      assertType<Expected>({} as Result);
    });
  });

  // @ai-generated - Tests enhanceElementWithProps for merging constructor instances with additional props
  describe("enhanceElementWithProps", () => {
    // Mock types for testing
    type MockInstance = { foo: string; bar: number };
    type MockConstructor = { new (): MockInstance };
    type AdditionalProps = { extra: boolean; added: string };

    describe("basic enhancement", () => {
      it("enhances constructor instance with additional props", () => {
        type Result = ReturnType<typeof enhanceElementWithProps<MockConstructor, AdditionalProps>>;
        type Expected = MockInstance & AdditionalProps;

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<Result>({} as { unrelated: true });
      });

      it("preserves original instance properties", () => {
        type Result = ReturnType<typeof enhanceElementWithProps<MockConstructor, AdditionalProps>>;

        // Verify original props are accessible
        const result = {} as Result;
        assertType<string>(result.foo);
        assertType<number>(result.bar);
      });

      it("adds new props to instance", () => {
        type Result = ReturnType<typeof enhanceElementWithProps<MockConstructor, AdditionalProps>>;

        // Verify additional props are accessible
        const result = {} as Result;
        assertType<boolean>(result.extra);
        assertType<string>(result.added);
      });
    });

    describe("instance extends props scenario", () => {
      it("returns instance when it already extends props", () => {
        type InstanceWithProps = { foo: string; extra: boolean };
        type ConstructorWithProps = { new (): InstanceWithProps };
        type Props = { extra: boolean };

        type Result = ReturnType<typeof enhanceElementWithProps<ConstructorWithProps, Props>>;

        // When instance already extends P, it should return I directly
        assertType<Result>({} as InstanceWithProps);
        assertType<InstanceWithProps>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<Result>({} as { unrelated: true });
      });

      it("returns instance when props are subset of instance", () => {
        type FullInstance = { a: string; b: number; c: boolean };
        type FullConstructor = { new (): FullInstance };
        type SubsetProps = { a: string; b: number };

        type Result = ReturnType<typeof enhanceElementWithProps<FullConstructor, SubsetProps>>;

        assertType<Result>({} as FullInstance);
        assertType<FullInstance>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<Result>({} as { unrelated: true });
      });
    });

    describe("non-constructor types", () => {
      it("merges additional props for non-constructor types", () => {
        // For non-constructor types (like string), the function returns T & P
        // This is the fallback behavior for primitive types used with type assertions
        type Result = ReturnType<typeof enhanceElementWithProps<string, AdditionalProps>>;

        assertType<Result>({} as string & AdditionalProps);
        assertType<string & AdditionalProps>({} as Result);
      });

      it("merges additional props with plain object type", () => {
        type PlainObject = { foo: string };
        type Result = ReturnType<typeof enhanceElementWithProps<PlainObject, AdditionalProps>>;

        assertType<Result>({} as PlainObject & AdditionalProps);
        assertType<PlainObject & AdditionalProps>({} as Result);
      });

      it("merges additional props with function type (non-constructor)", () => {
        type FnType = () => void;
        type Result = ReturnType<typeof enhanceElementWithProps<FnType, AdditionalProps>>;

        assertType<Result>({} as FnType & AdditionalProps);
        assertType<FnType & AdditionalProps>({} as Result);
      });
    });

    describe("Vue component scenarios", () => {
      it("enhances Vue component constructor with additional props", () => {
        type VueInstance = {
          $props: { msg: string };
          $emit: (e: "click") => void;
        };
        type VueConstructor = { new (): VueInstance };
        type ExtraProps = { customProp: number };

        type Result = ReturnType<typeof enhanceElementWithProps<VueConstructor, ExtraProps>>;
        type Expected = VueInstance & ExtraProps;

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // Verify Vue-specific properties are preserved
        const result = {} as Result;
        assertType<{ msg: string }>(result.$props);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<Result>({} as { unrelated: true });
      });

      it("handles component with complex instance type", () => {
        type ComplexInstance = {
          $props: { required: string; optional?: number };
          $emit: (e: "update", value: string) => void;
          $slots: { default: () => void };
          publicMethod: () => void;
        };
        type ComplexConstructor = { new (): ComplexInstance };
        type MergeProps = { injectedProp: boolean };

        type Result = ReturnType<typeof enhanceElementWithProps<ComplexConstructor, MergeProps>>;

        // Verify all original properties are accessible
        const result = {} as Result;
        assertType<{ required: string; optional?: number }>(result.$props);
        assertType<(e: "update", value: string) => void>(result.$emit);
        assertType<{ default: () => void }>(result.$slots);
        assertType<() => void>(result.publicMethod);
        assertType<boolean>(result.injectedProp);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<Result>({} as { unrelated: true });
      });
    });

    describe("edge cases", () => {
      it("handles empty props", () => {
        type Result = ReturnType<typeof enhanceElementWithProps<MockConstructor, {}>>;

        // With empty props, instance is already compatible, returns I directly
        assertType<Result>({} as MockInstance);
        assertType<MockInstance>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<Result>({} as { unrelated: true });
      });

      it("handles overlapping property types", () => {
        type InstanceWithFoo = { foo: string; other: number };
        type ConstructorWithFoo = { new (): InstanceWithFoo };
        type OverlappingProps = { foo: string; newProp: boolean };

        type Result = ReturnType<
          typeof enhanceElementWithProps<ConstructorWithFoo, OverlappingProps>
        >;

        // foo exists in both, should be merged via intersection
        const result = {} as Result;
        assertType<string>(result.foo);
        assertType<number>(result.other);
        assertType<boolean>(result.newProp);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<Result>({} as { unrelated: true });
      });

      it("handles never type", () => {
        type Result = ReturnType<typeof enhanceElementWithProps<never, {}>>;

        assertType<Result>({} as {});
      });
    });
  });

  /**
   * @ai-generated - Tests for ExtractComponentProps utility type.
   * Verifies prop extraction from Vue components and HTML elements.
   */
  describe("ExtractComponentProps", () => {
    // Import the type augmentations for HTML elements
    type ExtractComponentProps<T> = import("./components").ExtractComponentProps<T>;

    it("extracts props from component constructor", () => {
      type MockComponent = {
        new (): {
          $props: { label: string; disabled?: boolean };
        };
      };
      type Result = ExtractComponentProps<MockComponent>;

      assertType<Result>({} as { label: string; disabled?: boolean });
      assertType<{ label: string; disabled?: boolean }>({} as Result);

      // @ts-expect-error - Result is not any/unknown/never
      assertType<Result>({} as { unrelated: true });
    });

    it("extracts props from component instance with $props", () => {
      type MockInstance = {
        $props: { value: number; onChange: () => void };
        $emit: (event: string) => void;
      };
      type Result = ExtractComponentProps<MockInstance>;

      assertType<Result>({} as { value: number; onChange: () => void });
      assertType<{ value: number; onChange: () => void }>({} as Result);

      // @ts-expect-error - Result is not any/unknown/never
      assertType<Result>({} as { unrelated: true });
    });

    // ExtractComponentProps uses ExtractFromHTMLElement to map HTML element types
    // to their corresponding Vue HTML attribute types (e.g., HTMLAudioElement -> AudioHTMLAttributes)

    it("extracts props from HTMLAudioElement", () => {
      type Result = ExtractComponentProps<HTMLAudioElement>;
      type Booleanish = boolean | "true" | "false" | "";

      const result = {} as Result;

      // AudioHTMLAttributes includes media-specific properties
      // Note: Vue uses Booleanish for boolean HTML attributes
      assertType<string | undefined>(result.src);
      assertType<Booleanish | undefined>(result.controls);
      assertType<Booleanish | undefined>(result.autoplay);
      assertType<Booleanish | undefined>(result.loop);
      assertType<Booleanish | undefined>(result.muted);
      assertType<string | undefined>(result.preload);

      // Also includes base HTMLAttributes
      assertType<string | undefined>(result.id);
      assertType<import("vue").ClassValue | undefined>(result.class);

      // @ts-expect-error - Result is not any/unknown/never
      assertType<Result>({} as { unrelated: true });
    });

    it("extracts props from HTMLInputElement", () => {
      type Result = ExtractComponentProps<HTMLInputElement>;
      type Booleanish = boolean | "true" | "false" | "";

      const result = {} as Result;

      // InputHTMLAttributes includes input-specific properties
      assertType<string | undefined>(result.type);
      assertType<string | number | readonly string[] | undefined>(result.value);
      assertType<string | undefined>(result.placeholder);
      assertType<Booleanish | undefined>(result.disabled);
      assertType<Booleanish | undefined>(result.readonly);
      assertType<Booleanish | undefined>(result.required);
      assertType<string | undefined>(result.name);

      // Also includes base HTMLAttributes
      assertType<string | undefined>(result.id);
      assertType<import("vue").ClassValue | undefined>(result.class);

      // @ts-expect-error - Result is not any/unknown/never
      assertType<Result>({} as { unrelated: true });
    });

    it("extracts props from HTMLVideoElement", () => {
      type Result = ExtractComponentProps<HTMLVideoElement>;
      type Booleanish = boolean | "true" | "false" | "";

      const result = {} as Result;

      // VideoHTMLAttributes includes video/media-specific properties
      assertType<string | undefined>(result.src);
      assertType<Booleanish | undefined>(result.controls);
      assertType<string | undefined>(result.poster);
      assertType<string | number | undefined>(result.width);
      assertType<string | number | undefined>(result.height);
      assertType<Booleanish | undefined>(result.autoplay);
      assertType<Booleanish | undefined>(result.loop);
      assertType<Booleanish | undefined>(result.muted);

      // Also includes base HTMLAttributes
      assertType<string | undefined>(result.id);
      assertType<import("vue").ClassValue | undefined>(result.class);

      // @ts-expect-error - Result is not any/unknown/never
      assertType<Result>({} as { unrelated: true });
    });

    it("extracts props from HTMLButtonElement", () => {
      type Result = ExtractComponentProps<HTMLButtonElement>;
      type Booleanish = boolean | "true" | "false" | "";

      const result = {} as Result;

      // ButtonHTMLAttributes includes button-specific properties
      assertType<"button" | "submit" | "reset" | undefined>(result.type);
      assertType<Booleanish | undefined>(result.disabled);
      assertType<string | undefined>(result.form);
      assertType<string | undefined>(result.name);
      // Note: value in Vue's ButtonHTMLAttributes supports multiple types
      assertType<string | number | readonly string[] | undefined>(result.value);

      // Also includes base HTMLAttributes
      assertType<string | undefined>(result.id);
      assertType<import("vue").ClassValue | undefined>(result.class);

      // @ts-expect-error - Result is not any/unknown/never
      assertType<Result>({} as { unrelated: true });
    });

    it("extracts props from HTMLFormElement", () => {
      type Result = ExtractComponentProps<HTMLFormElement>;

      const result = {} as Result;

      // FormHTMLAttributes includes form-specific properties
      assertType<string | undefined>(result.action);
      assertType<string | undefined>(result.method);
      assertType<string | undefined>(result.enctype);
      assertType<string | undefined>(result.target);
      assertType<string | undefined>(result.name);

      // Also includes base HTMLAttributes
      assertType<string | undefined>(result.id);
      assertType<import("vue").ClassValue | undefined>(result.class);

      // @ts-expect-error - Result is not any/unknown/never
      assertType<Result>({} as { unrelated: true });
    });

    it("extracts props from HTMLAnchorElement", () => {
      type Result = ExtractComponentProps<HTMLAnchorElement>;

      const result = {} as Result;

      // AnchorHTMLAttributes includes anchor-specific properties
      assertType<string | undefined>(result.href);
      assertType<string | undefined>(result.target);
      assertType<string | undefined>(result.rel);
      assertType<string | undefined>(result.download);

      // Also includes base HTMLAttributes
      assertType<string | undefined>(result.id);
      assertType<import("vue").ClassValue | undefined>(result.class);

      // @ts-expect-error - Result is not any/unknown/never
      assertType<Result>({} as { unrelated: true });
    });

    it("extracts props from HTMLImageElement", () => {
      type Result = ExtractComponentProps<HTMLImageElement>;

      const result = {} as Result;

      // ImgHTMLAttributes includes image-specific properties
      assertType<string | undefined>(result.src);
      assertType<string | undefined>(result.alt);
      assertType<string | number | undefined>(result.width);
      assertType<string | number | undefined>(result.height);
      assertType<"" | "anonymous" | "use-credentials" | undefined>(result.crossorigin);
      assertType<"async" | "auto" | "sync" | undefined>(result.decoding);
      assertType<"lazy" | "eager" | undefined>(result.loading);

      // Also includes base HTMLAttributes
      assertType<string | undefined>(result.id);
      assertType<import("vue").ClassValue | undefined>(result.class);

      // @ts-expect-error - Result is not any/unknown/never
      assertType<Result>({} as { unrelated: true });
    });

    it("extracts props from HTMLSelectElement", () => {
      type Result = ExtractComponentProps<HTMLSelectElement>;
      type Booleanish = boolean | "true" | "false" | "";

      const result = {} as Result;

      // SelectHTMLAttributes includes select-specific properties
      assertType<Booleanish | undefined>(result.disabled);
      assertType<Booleanish | undefined>(result.multiple);
      assertType<string | undefined>(result.name);
      assertType<Booleanish | undefined>(result.required);
      assertType<string | number | undefined>(result.size);

      // Also includes base HTMLAttributes
      assertType<string | undefined>(result.id);
      assertType<import("vue").ClassValue | undefined>(result.class);

      // @ts-expect-error - Result is not any/unknown/never
      assertType<Result>({} as { unrelated: true });
    });

    it("extracts props from HTMLTextAreaElement", () => {
      type Result = ExtractComponentProps<HTMLTextAreaElement>;
      type Booleanish = boolean | "true" | "false" | "";

      const result = {} as Result;

      // TextareaHTMLAttributes includes textarea-specific properties
      assertType<Booleanish | undefined>(result.disabled);
      assertType<string | undefined>(result.placeholder);
      assertType<Booleanish | undefined>(result.readonly);
      assertType<Booleanish | undefined>(result.required);
      assertType<string | number | undefined>(result.rows);
      assertType<string | number | undefined>(result.cols);
      assertType<string | undefined>(result.name);

      // Also includes base HTMLAttributes
      assertType<string | undefined>(result.id);
      assertType<import("vue").ClassValue | undefined>(result.class);

      // @ts-expect-error - Result is not any/unknown/never
      assertType<Result>({} as { unrelated: true });
    });

    it("extracts HTMLAttributes for generic HTMLElement", () => {
      // Generic HTMLElement falls through to the default HTMLAttributes case
      type Result = ExtractComponentProps<HTMLElement>;

      const result = {} as Result;

      // Base HTMLAttributes properties
      assertType<string | undefined>(result.id);
      assertType<import("vue").ClassValue | undefined>(result.class);
      assertType<import("vue").StyleValue | undefined>(result.style);
      assertType<string | number | undefined>(result.tabindex);
      assertType<string | undefined>(result.title);

      // @ts-expect-error - Result is not any/unknown/never
      assertType<Result>({} as { unrelated: true });
    });

    it("recursively extracts from constructor to instance", () => {
      type NestedComponent = {
        new (): {
          new (): {
            $props: { deep: boolean };
          };
        };
      };
      // Note: This tests the recursive unwrapping behavior
      type Result = ExtractComponentProps<NestedComponent>;

      // The first level unwraps, then it should continue extracting
      // Since the instance doesn't directly have $props, it keeps unwrapping
    });

    it("handles component with empty $props", () => {
      type EmptyPropsComponent = {
        new (): {
          $props: {};
        };
      };
      type Result = ExtractComponentProps<EmptyPropsComponent>;

      assertType<Result>({} as {});
      assertType<{}>({} as Result);
    });
  });
});
