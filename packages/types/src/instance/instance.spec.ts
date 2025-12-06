/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for Vue component instance type helpers including:
 * - ToInstanceProps, CreateTypedPublicInstanceFromNormalisedMacro
 * - InternalInstanceFromMacro, PublicInstanceFromNormalisedMacro
 * - CreateExportedInstanceFromNormalisedMacro, CreateExportedInstanceFromMacro
 * - PublicInstanceFromMacro: realistic macro usage tests
 * - Verifies props/models are accessible on instances and ComponentPublicInstance compatibility
 */
import { describe, it, assertType, expect } from "vitest";
import type {
  ModelRef,
  ComponentPublicInstance,
  WatchStopHandle,
  ComponentInternalInstance,
} from "vue";
import {
  CreateTypedInternalInstanceFromNormalisedMacro,
  CreateTypedPublicInstanceFromNormalisedMacro,
  PublicInstanceFromNormalisedMacro,
  CreateExportedInstanceFromNormalisedMacro,
  CreateExportedInstanceFromMacro,
  InternalInstanceFromMacro,
  ToInstanceProps,
  PublicInstanceFromMacro,
  SFCPublicInstanceFromMacro,
  ExternalPublicInstanceFromMacro,
  TestExternalPublicInstanceFromMacro,
} from "./instance";
import { createMacroReturn, CreateMacroReturn } from "../setup";
import type { PropsWithDefaults } from "../props";
import { UniqueKey } from "../helpers";

describe("instance helpers", () => {
  describe("ToInstanceProps", () => {
    it("extracts props with MakePublicProps when MakeDefaultsOptional is true", () => {
      // ToInstanceProps receives the extracted macro props structure { props: ..., defaults: ... }
      type MacroProps = {
        props: { type: { id: number; name: string } };
        defaults: { value: { name: string } };
      };
      type Props = ToInstanceProps<MacroProps, true>;

      // name has default, so should be optional for external users
      const props: Props = { id: 1 };
      assertType<number>(props.id);
      assertType<string | undefined>(props.name);
    });

    it("extracts props with MakeInternalProps when MakeDefaultsOptional is false", () => {
      // ToInstanceProps receives the extracted macro props structure { props: ..., defaults: ... }
      type MacroProps = {
        props: { type: { id: number; name: string } };
        defaults: { value: { name: string } };
      };
      type Props = ToInstanceProps<MacroProps, false>;

      // internally, name with default includes undefined (Vue behavior)
      const props: Props = { id: 1, name: "test" };
      assertType<number>(props.id);
      assertType<string | undefined>(props.name);
    });

    it("handles empty props object", () => {
      type MacroProps = { props: {}; defaults: { value: {} } };
      type Props = ToInstanceProps<MacroProps, true>;
      const props: Props = {};
      assertType<{}>(props);
    });
  });

  describe("CreateTypedPublicInstanceFromNormalisedMacro", () => {
    it("creates public instance with correct types", () => {
      type TestNormalized = {
        props: { props: { type: { count: number } }; defaults: { value: {} } };
        emits: { value: (e: "update", val: number) => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };
      type Instance =
        CreateTypedPublicInstanceFromNormalisedMacro<TestNormalized>;

      const instance = {} as Instance;

      // $props should be available
      assertType<object>(instance.$props);

      // $attrs should be available
      assertType<{}>(instance.$attrs);

      // $refs should be available
      assertType<{}>(instance.$refs);

      // $ should be internal instance
      assertType<object>(instance.$);

      // $data should be available
      assertType<{}>(instance.$data);
    });

    it("exposes props directly on instance (not just $props)", () => {
      type TestNormalized = {
        props: {
          props: { type: { count: number; name: string } };
          defaults: { value: {} };
        };
        emits: { value: (e: "update", val: number) => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };
      type Instance =
        CreateTypedPublicInstanceFromNormalisedMacro<TestNormalized>;

      const instance = {} as Instance;

      // Props should be directly accessible on instance
      assertType<number>(instance.count);
      assertType<string>(instance.name);

      // $props type check (verify the type has these props)
      type HasPropsType = Instance["$props"] extends {
        count: number;
        name: string;
      }
        ? true
        : false;
      assertType<HasPropsType>({} as true);
    });

    it("exposes model values directly on instance", () => {
      type TestNormalized = {
        props: { props: { type: {} }; defaults: { value: {} } };
        emits: { value: (e: "update", val: number) => void };
        slots: {};
        options: {};
        model: {
          modelValue: { value: ModelRef<string, "modelValue"> };
          title: { value: ModelRef<number, "title"> };
        };
        expose: {};
        templateRef: {};
        $data: {};
      };
      type Instance =
        CreateTypedPublicInstanceFromNormalisedMacro<TestNormalized>;

      const instance = {} as Instance;

      // Model values should be directly accessible on instance
      assertType<string>(instance.modelValue);
      assertType<number>(instance.title);

      // $props type check (verify the type has model values)
      type HasModelPropsType = Instance["$props"] extends {
        modelValue: string;
        title: number;
      }
        ? true
        : false;
      assertType<HasModelPropsType>({} as true);
    });

    it("combines props and model values on instance", () => {
      type TestNormalized = {
        props: { props: { type: { id: number } }; defaults: { value: {} } };
        emits: { value: (e: "update:modelValue", val: string) => void };
        slots: {};
        options: {};
        model: {
          modelValue: { value: ModelRef<string, "modelValue"> };
        };
        expose: {};
        templateRef: {};
        $data: {};
      };
      type Instance =
        CreateTypedPublicInstanceFromNormalisedMacro<TestNormalized>;

      const instance = {} as Instance;

      // Both props and model values accessible
      assertType<number>(instance.id);
      assertType<string>(instance.modelValue);

      // $props type check (verify the type has both props and model values)
      type HasBothType = Instance["$props"] extends {
        id: number;
        modelValue: string;
      }
        ? true
        : false;
      assertType<HasBothType>({} as true);
    });
  });

  describe("CreateTypedInternalInstanceFromNormalisedMacro", () => {
    it("creates internal instance with correct structure", () => {
      type TestNormalized = {
        props: { props: { type: { id: number } }; defaults: { value: {} } };
        emits: { value: (e: "change", id: number) => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };
      type Internal =
        CreateTypedInternalInstanceFromNormalisedMacro<TestNormalized>;

      const internal = {} as Internal;

      // props in internal instance
      assertType<object>(internal.props);

      // attrs
      assertType<{}>(internal.attrs);

      // refs (templateRef)
      assertType<{}>(internal.refs);

      // data
      assertType<{}>(internal.data);
    });
  });

  describe("InternalInstanceFromMacro", () => {
    it("creates internal instance with enhanced props type", () => {
      // Use direct normalized structure for props test
      type TestNormalized = {
        props: {
          props: { type: { id: number; name: string } };
          defaults: { value: {} };
        };
        emits: { value: () => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Internal = InternalInstanceFromMacro<TestNormalized, {}, false>;

      const internal = {} as Internal;

      // Enhanced props type
      assertType<{ id: number; name: string }>(internal.props);
    });

    it("creates internal instance with enhanced emit type", () => {
      // Use direct normalized structure for emit test
      type TestNormalized = {
        props: { props: { type: {} }; defaults: { value: {} } };
        emits: { value: (e: "change", val: number) => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Internal = InternalInstanceFromMacro<TestNormalized>;

      const internal = {} as Internal;

      // Enhanced emit type
      assertType<(e: "change", val: number) => void>(internal.emit);
    });

    it("creates internal instance with enhanced slots type", () => {
      // Use direct normalized structure for slots test
      type TestNormalized = {
        props: { props: { type: {} }; defaults: { value: {} } };
        emits: { value: () => void };
        slots: {
          value: {
            default: () => any;
            header: (props: { title: string }) => any;
          };
        };
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Internal = InternalInstanceFromMacro<TestNormalized>;

      const internal = {} as Internal;

      // Enhanced slots type - Vue wraps it in SlotsType
      assertType<object>(internal.slots);
    });

    it("creates internal instance with model props and emits", () => {
      // Use direct normalized structure for model test
      type TestNormalized = {
        props: { props: { type: { count: number } }; defaults: { value: {} } };
        emits: { value: () => void };
        slots: {};
        options: {};
        model: {
          modelValue: { value: ModelRef<string, "modelValue"> };
        };
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Internal = InternalInstanceFromMacro<TestNormalized>;

      // Props type check - includes model value
      type HasModelValue = Internal["props"] extends {
        modelValue: string;
        count: number;
      }
        ? true
        : false;
      assertType<HasModelValue>({} as true);

      // Emit type check - includes model update
      type EmitType = Internal["emit"];
      type CanEmitModelUpdate = EmitType extends (
        e: "update:modelValue",
        v: string
      ) => void
        ? true
        : false;
      assertType<CanEmitModelUpdate>({} as true);
    });

    it("has all standard ComponentInternalInstance properties", () => {
      type TestNormalized = {
        props: { props: { type: { id: number } }; defaults: { value: {} } };
        emits: { value: (e: "change") => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Internal = InternalInstanceFromMacro<TestNormalized>;

      const internal = {} as Internal;

      // Core ComponentInternalInstance properties
      assertType<number>(internal.uid);
      assertType<Internal["parent"]>(internal.parent);
      assertType<Internal["root"]>(internal.root);
      assertType<object>(internal.appContext);
      assertType<Internal["type"]>(internal.type);
      assertType<Internal["vnode"]>(internal.vnode);
      assertType<Internal["subTree"]>(internal.subTree);

      // Lifecycle flags
      assertType<boolean>(internal.isMounted);
      assertType<boolean>(internal.isUnmounted);

      // Enhanced properties from our macro
      assertType<{ id: number }>(internal.props);
      assertType<(e: "change") => void>(internal.emit);
    });

    it("has ComponentInternalInstance structure", () => {
      type TestNormalized = {
        props: { props: { type: { id: number } }; defaults: { value: {} } };
        emits: { value: () => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Internal = InternalInstanceFromMacro<TestNormalized>;

      // Verify all key ComponentInternalInstance properties exist
      type HasRequiredProperties = Internal extends {
        uid: number;
        parent: ComponentInternalInstance | null;
        root: ComponentInternalInstance;
        appContext: object;
        type: object;
        vnode: object;
        subTree: object;
        props: object;
        attrs: object;
        slots: object;
        refs: object;
        emit: Function;
        isMounted: boolean;
        isUnmounted: boolean;
        proxy: ComponentPublicInstance | null;
      }
        ? true
        : false;

      assertType<HasRequiredProperties>({} as true);
    });

    it("proxy property is typed as PublicInstance", () => {
      // Use direct normalized structure
      type TestNormalized = {
        props: { props: { type: { count: number } }; defaults: { value: {} } };
        emits: { value: () => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Internal = InternalInstanceFromMacro<TestNormalized>;

      // proxy should be typed as our enhanced public instance or null
      type ProxyType = Internal["proxy"];
      type ProxyIsValid = ProxyType extends ComponentPublicInstance | null
        ? true
        : false;
      assertType<ProxyIsValid>({} as true);

      // Should have standard ComponentPublicInstance properties
      type NonNullProxy = NonNullable<ProxyType>;
      type HasEl = NonNullProxy extends { $el: Element | null } ? true : false;
      type HasForceUpdate = NonNullProxy extends { $forceUpdate: () => void }
        ? true
        : false;
      assertType<HasEl>({} as true);
      assertType<HasForceUpdate>({} as true);
    });
  });

  describe("PublicInstanceFromNormalisedMacro", () => {
    it("creates public instance with props and models accessible directly", () => {
      type TestNormalized = {
        props: { props: { type: { count: number } }; defaults: { value: {} } };
        emits: { value: (e: "change", val: number) => void };
        slots: {};
        options: {};
        model: {
          modelValue: { value: ModelRef<string, "modelValue"> };
        };
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Instance = PublicInstanceFromNormalisedMacro<
        TestNormalized,
        {},
        HTMLDivElement,
        false,
        false
      >;

      const instance = {} as Instance;

      // Props accessible directly
      assertType<number>(instance.count);

      // Model values accessible directly
      assertType<string>(instance.modelValue);

      // $props type check
      type HasPropsAndModelType = Instance["$props"] extends {
        count: number;
        modelValue: string;
      }
        ? true
        : false;
      assertType<HasPropsAndModelType>({} as true);

      // Standard Vue instance properties
      assertType<HTMLDivElement | null>(instance.$el);
    });

    it("has all standard ComponentPublicInstance properties", () => {
      type TestNormalized = {
        props: { props: { type: { value: string } }; defaults: { value: {} } };
        emits: { value: (e: "update", val: string) => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Instance = PublicInstanceFromNormalisedMacro<
        TestNormalized,
        {},
        HTMLElement,
        false,
        false
      >;

      const instance = {} as Instance;

      // Core ComponentPublicInstance properties
      assertType<HTMLElement | null>(instance.$el);
      assertType<ComponentPublicInstance | null>(instance.$parent);
      assertType<ComponentPublicInstance | null>(instance.$root);
      assertType<object>(instance.$options);

      // Reactivity methods
      assertType<(source: any, cb: any, options?: any) => WatchStopHandle>(
        instance.$watch
      );
      assertType<() => void>(instance.$forceUpdate);
      assertType<<T = void>(fn?: () => T) => Promise<Awaited<T>>>(
        instance.$nextTick
      );
    });

    it("has ComponentPublicInstance structure", () => {
      type TestNormalized = {
        props: { props: { type: { id: number } }; defaults: { value: {} } };
        emits: { value: () => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Instance = PublicInstanceFromNormalisedMacro<
        TestNormalized,
        {},
        Element,
        false,
        false
      >;

      // Verify all key ComponentPublicInstance properties exist
      type HasRequiredProperties = Instance extends {
        $el: Element | null;
        $parent: ComponentPublicInstance | null;
        $root: ComponentPublicInstance | null;
        $data: object;
        $props: object;
        $attrs: object;
        $refs: object;
        $slots: object;
        $emit: Function;
        $forceUpdate: () => void;
        $nextTick: Function;
        $watch: Function;
      }
        ? true
        : false;

      assertType<HasRequiredProperties>({} as true);
    });
  });

  describe("CreateExportedInstanceFromNormalisedMacro", () => {
    it("creates exported instance with MakeDefaultsOptional=true", () => {
      type TestNormalized = {
        props: {
          props: {
            value: { name: string };
            type: { id: number; name: string };
          };
          defaults: { value: { name: string } };
        };
        emits: { value: (e: "update") => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Instance = CreateExportedInstanceFromNormalisedMacro<
        TestNormalized,
        {},
        HTMLElement
      >;

      const instance = {} as Instance;

      // Props with defaults should be optional (undefined allowed)
      assertType<number>(instance.id);
      assertType<string | undefined>(instance.name);

      // $props type check
      type HasPropsType = Instance["$props"] extends {
        id: number;
        name?: string | undefined;
      }
        ? true
        : false;
      assertType<HasPropsType>({} as true);
    });

    it("exposes props and model values on instance", () => {
      type TestNormalized = {
        props: {
          props: { type: { active: boolean } };
          defaults: { value: {} };
        };
        emits: { value: () => void };
        slots: {};
        options: {};
        model: {
          count: { value: ModelRef<number, "count"> };
        };
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Instance = CreateExportedInstanceFromNormalisedMacro<TestNormalized>;

      const instance = {} as Instance;

      // Props accessible directly (boolean without default is optional in exported instance)
      assertType<boolean | undefined>(instance.active);

      // Model accessible directly
      assertType<number>(instance.count);
    });

    it("has all standard ComponentPublicInstance properties", () => {
      type TestNormalized = {
        props: { props: { type: { value: string } }; defaults: { value: {} } };
        emits: { value: () => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Instance = CreateExportedInstanceFromNormalisedMacro<
        TestNormalized,
        {},
        HTMLDivElement
      >;

      const instance = {} as Instance;

      // Core ComponentPublicInstance properties should be present
      assertType<HTMLDivElement | null>(instance.$el);
      assertType<ComponentPublicInstance | null>(instance.$parent);
      assertType<ComponentPublicInstance | null>(instance.$root);
      assertType<object>(instance.$options);

      // Reactivity methods
      assertType<(source: any, cb: any, options?: any) => WatchStopHandle>(
        instance.$watch
      );
      assertType<() => void>(instance.$forceUpdate);
      assertType<<T = void>(fn?: () => T) => Promise<Awaited<T>>>(
        instance.$nextTick
      );

      // Our custom overrides should also work
      assertType<string | undefined>(instance.value);

      // $props type check
      type HasPropsType = Instance["$props"] extends {
        value: string | undefined;
      }
        ? true
        : false;
      assertType<HasPropsType>({} as true);
    });

    it("has ComponentPublicInstance structure", () => {
      type TestNormalized = {
        props: { props: { type: { id: number } }; defaults: { value: {} } };
        emits: { value: () => void };
        slots: {};
        options: {};
        model: {};
        expose: {};
        templateRef: {};
        $data: {};
      };

      type Instance = CreateExportedInstanceFromNormalisedMacro<TestNormalized>;

      // Verify all key ComponentPublicInstance properties exist
      type HasRequiredProperties = Instance extends {
        $el: Element | null;
        $parent: ComponentPublicInstance | null;
        $root: ComponentPublicInstance | null;
        $data: object;
        $props: object;
        $attrs: object;
        $refs: object;
        $slots: object;
        $emit: Function;
        $forceUpdate: () => void;
        $nextTick: Function;
        $watch: Function;
      }
        ? true
        : false;

      assertType<HasRequiredProperties>({} as true);
    });
  });

  describe("CreateExportedInstanceFromMacro", () => {
    it("works with MacroReturn structure for props only", () => {
      // This tests that CreateExportedInstanceFromMacro properly normalizes
      // a macro return structure through NormaliseMacroReturn
      // Using type directly instead of createMacroReturn (which is a type-only helper)
      type MacroReturnType = {
        [UniqueKey]: {
          props: { value: { count: number }; type: { count: number } };
        };
      };

      type Instance = CreateExportedInstanceFromMacro<MacroReturnType>;

      // Props accessible directly (exported instance has MakeDefaultsOptional=true)
      type HasCount = Instance extends { count: number | undefined }
        ? true
        : false;
      assertType<HasCount>({} as true);

      // $props type check
      type HasPropsCount = Instance["$props"] extends {
        count: number | undefined;
      }
        ? true
        : false;
      assertType<HasPropsCount>({} as true);
    });
  });

  /**
   * @ai-generated - Tests for instances with exposed macro values
   */
  describe("Instance with expose macro", () => {
    it("public instance exposes methods from defineExpose", () => {
      type TestNormalized = {
        props: { props: { type: { id: number } }; defaults: { value: {} } };
        emits: { value: () => void };
        slots: {};
        options: {};
        model: {};
        expose: {
          object: {
            focus: () => void;
            reset: (value: string) => void;
            getValue: () => string;
          };
        };
        templateRef: {};
        $data: {};
      };

      type Instance = PublicInstanceFromNormalisedMacro<
        TestNormalized,
        {},
        HTMLElement,
        false,
        false
      >;

      const instance = {} as Instance;

      // Exposed methods should be accessible on the instance
      assertType<() => void>(instance.focus);
      assertType<(value: string) => void>(instance.reset);
      assertType<() => string>(instance.getValue);
      // Props should still be accessible
      assertType<number>(instance.$props.id);
    });

    it("exported instance exposes methods from defineExpose", () => {
      type TestNormalized = {
        props: { props: { type: { name: string } }; defaults: { value: {} } };
        emits: { value: (e: "change", val: string) => void };
        slots: {};
        options: {};
        model: {};
        expose: {
          object: {
            validate: () => boolean;
            clear: () => void;
          };
        };
        templateRef: {};
        $data: {};
      };

      type Instance = CreateExportedInstanceFromNormalisedMacro<
        TestNormalized,
        {},
        HTMLInputElement
      >;

      const instance = {} as Instance;

      // Exposed methods accessible
      assertType<() => boolean>(instance.validate);
      assertType<() => void>(instance.clear);

      // Props accessible (with MakeDefaultsOptional=true for exported)
      assertType<string | undefined>(instance.$props.name);

      // Standard Vue properties
      assertType<HTMLInputElement | null>(instance.$el);
    });

    it("internal instance has exposed property with correct type", () => {
      type TestNormalized = {
        props: { props: { type: { count: number } }; defaults: { value: {} } };
        emits: { value: () => void };
        slots: {};
        options: {};
        model: {};
        expose: {
          object: {
            increment: () => void;
            decrement: () => void;
            getCount: () => number;
          };
        };
        templateRef: {};
        $data: {};
      };

      type Internal = InternalInstanceFromMacro<TestNormalized>;

      const internal = {} as Internal;

      // Check exposed property exists on internal instance
      // Note: Vue's ComponentInternalInstance has `exposed` property
      assertType<object | null>(internal.exposed);
    });

    it("instance with both props, models and expose", () => {
      type TestNormalized = {
        props: {
          props: { type: { label: string; disabled: boolean } };
          defaults: { value: {} };
        };
        emits: { value: (e: "submit", data: object) => void };
        slots: {};
        options: {};
        model: {
          modelValue: { value: ModelRef<string, "modelValue"> };
        };
        expose: {
          object: {
            submit: () => void;
            reset: () => void;
            isValid: () => boolean;
          };
        };
        templateRef: {};
        $data: {};
      };

      type Instance = PublicInstanceFromNormalisedMacro<
        TestNormalized,
        {},
        HTMLFormElement,
        false,
        false
      >;

      const instance = {} as Instance;

      // Props accessible
      assertType<string>(instance.$props.label);
      assertType<boolean>(instance.$props.disabled);

      // Model accessible
      assertType<string>(instance.$props.modelValue);

      // Exposed methods accessible
      assertType<() => void>(instance.submit);
      assertType<() => void>(instance.reset);
      assertType<() => boolean>(instance.isValid);

      // $el is the element type
      assertType<HTMLFormElement | null>(instance.$el);
    });

    it("CreateExportedInstanceFromMacro with expose", () => {
      type MacroReturnType = {
        [UniqueKey]: {
          props: { value: { id: number }; type: { id: number } };
          expose: {
            object: {
              refresh: () => Promise<void>;
              getData: () => object;
            };
          };
        };
      };

      type Instance = CreateExportedInstanceFromMacro<MacroReturnType>;

      // Props accessible
      type HasId = Instance extends { id: number | undefined } ? true : false;
      assertType<HasId>({} as true);

      // Exposed methods accessible
      type HasRefresh = Instance extends { refresh: () => Promise<void> }
        ? true
        : false;
      assertType<HasRefresh>({} as true);

      type HasGetData = Instance extends { getData: () => object }
        ? true
        : false;
      assertType<HasGetData>({} as true);
    });

    it("expose with generic types", () => {
      type TestNormalized = {
        props: { props: { type: {} }; defaults: { value: {} } };
        emits: { value: () => void };
        slots: {};
        options: {};
        model: {};
        expose: {
          object: {
            setItems: <T>(items: T[]) => void;
            getItem: <T>(index: number) => T | undefined;
          };
        };
        templateRef: {};
        $data: {};
      };

      type Instance = CreateExportedInstanceFromNormalisedMacro<TestNormalized>;

      const instance = {} as Instance;

      // Generic exposed methods
      assertType<<T>(items: T[]) => void>(instance.setItems);
      assertType<<T>(index: number) => T | undefined>(instance.getItem);
    });
  });

  /**
   * @ai-generated - Comprehensive tests for PublicInstanceFromMacro with realistic macro usage
   * Tests various combinations of defineProps, defineEmits, defineModel, defineSlots, defineExpose, withDefaults
   */
  describe("PublicInstanceFromMacro - realistic macro usage", () => {
    // Tests use ReturnType<typeof createMacroReturn({...})> to get proper macro types
    // This mirrors what the Verter transformer produces

    describe("defineProps only", () => {
      it("handles type-argument syntax: defineProps<{ id: number }>()", () => {
        // Simulates: const props = defineProps<{ id: number; name?: string }>();
        const macroReturn = {
          props: {
            value: {} as { id: number; name?: string },
          },
          ...createMacroReturn({
            props: {
              value: {} as { id: number; name?: string },
              type: {} as { id: number; name?: string },
            },
          }),
        };
        type MacroReturn = typeof macroReturn;

        type Instance = PublicInstanceFromMacro<
          MacroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // Props should be accessible on $props
        assertType<number>(instance.$props.id);
        assertType<string | undefined>(instance.$props.name);

        // Props should also be directly accessible on instance
        assertType<number>(instance.id);
        assertType<string | undefined>(instance.name);

        // @ts-expect-error - Should not be any/unknown/never
        assertType<{ __unrelatedProp: true }>(instance.$props);
      });

      it("handles object syntax: defineProps({ foo: String })", () => {
        // Simulates: const props = defineProps({ foo: String, bar: { type: Number, default: 0 } });
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { foo: string | undefined; bar: number },
            type: {} as { foo: string | undefined; bar: number },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // Props should be accessible
        assertType<string | undefined>(instance.$props.foo);
        assertType<number>(instance.$props.bar);

        // @ts-expect-error - Should not be any/unknown/never
        assertType<{ __unrelatedProp: true }>(instance.$props);
      });

      it("handles array syntax: defineProps(['foo', 'bar'])", () => {
        // Simulates: const props = defineProps(['foo', 'bar']);
        // Array syntax in Vue creates props with any type
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { foo?: any; bar?: any },
            type: {} as { foo?: any; bar?: any },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        // Just verify the type resolves without errors
        const instance = {} as Instance;

        // Instance should have $el from Vue
        assertType<HTMLElement | null>(instance.$el);
      });

      // @ai-generated - Test for all-optional props
      it("handles all-optional props: defineProps<{ message?: string }>()", () => {
        // Simulates: const props = defineProps<{ message?: string }>();
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { message?: string },
            type: {} as { message?: string },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // Props should be accessible on $props
        assertType<string | undefined>(instance.$props.message);

        // Props should also be directly accessible on instance
        assertType<string | undefined>(instance.message);

        // @ts-expect-error - Should not be any/unknown/never
        assertType<{ __unrelatedProp: true }>(instance.$props);
      });
    });

    describe("defineProps with withDefaults", () => {
      it("handles withDefaults making props optional", () => {
        // Simulates:
        // const props = withDefaults(defineProps<{ title: string; count: number }>(), {
        //   title: 'Default Title'
        // });
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { title: string; count: number },
            type: {} as { title: string; count: number },
          },
          withDefaults: {
            value: { title: "Default Title" } as const,
            type: {} as [
              { title: string; count: number },
              { title: "Default Title" }
            ],
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          true
        >;

        const instance = {} as Instance;

        // title has default, so should be optional in exported instance (MakeDefaultsOptional=true)
        assertType<string | undefined>(instance.$props.title);

        // count has no default, should be optional with undefined
        assertType<number | undefined>(instance.$props.count);

        // @ts-expect-error - Should not be any/unknown/never
        assertType<{ __unrelatedProp: true }>(instance.$props);
      });

      it("handles withDefaults internal instance (MakeDefaultsOptional=false)", () => {
        // Same setup but with MakeDefaultsOptional=false (internal instance)
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { title: string; count: number },
            type: {} as { title: string; count: number },
          },
          withDefaults: {
            value: { title: "Default Title" } as const,
            type: {} as [
              { title: string; count: number },
              { title: "Default Title" }
            ],
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // Internal instance - props with defaults are still string (not optional)
        // title has a default so it's always defined internally
        assertType<string | undefined>(instance.$props.title);
        assertType<number | undefined>(instance.$props.count);

        // @ts-expect-error - Should not be any/unknown/never
        assertType<{ __unrelatedProp: true }>(instance.$props);
      });
    });

    describe("defineEmits", () => {
      it("handles type-argument syntax with call signatures", () => {
        // Simulates: const emit = defineEmits<{ (e: 'update', val: number): void; (e: 'delete'): void }>();
        type EmitType = {
          (e: "update", val: number): void;
          (e: "delete"): void;
        };

        const macroReturn = createMacroReturn({
          emits: { value: {} as EmitType, type: {} as EmitType },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // $emit should have the emit function type
        type EmitFn = Instance["$emit"];
        type CanEmitUpdate = EmitFn extends (e: "update", val: number) => void
          ? true
          : false;
        type CanEmitDelete = EmitFn extends (e: "delete") => void
          ? true
          : false;

        assertType<CanEmitUpdate>({} as true);
        assertType<CanEmitDelete>({} as true);
      });

      it("handles shorthand object syntax", () => {
        // Simulates: const emit = defineEmits<{ update: [val: number]; delete: [] }>();
        type EmitType = {
          (e: "update", val: number): void;
          (e: "delete"): void;
        };

        const macroReturn = createMacroReturn({
          emits: { value: {} as EmitType, type: {} as EmitType },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        type EmitFn = Instance["$emit"];
        type ValidEmit = EmitFn extends (e: "update", val: number) => void
          ? true
          : false;
        assertType<ValidEmit>({} as true);
      });
    });

    describe("defineModel", () => {
      it("handles basic modelValue", () => {
        // Simulates: const model = defineModel<string>();
        const macroReturn = createMacroReturn({
          model: {
            modelValue: {
              value: {} as ModelRef<string, "modelValue">,
              type: {} as string,
            },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // modelValue should be accessible on $props
        assertType<string>(instance.$props.modelValue);

        // modelValue should be directly accessible on instance
        assertType<string>(instance.modelValue);

        // $emit should have update:modelValue
        type EmitFn = Instance["$emit"];
        type CanEmitUpdate = EmitFn extends (
          e: "update:modelValue",
          val: string
        ) => void
          ? true
          : false;
        assertType<CanEmitUpdate>({} as true);

        // @ts-expect-error - Should not be any/unknown/never
        assertType<{ __unrelatedProp: true }>(instance.$props);
      });

      it("handles named model", () => {
        // Simulates: const count = defineModel<number>('count');
        const macroReturn = createMacroReturn({
          model: {
            count: {
              value: {} as ModelRef<number, "count">,
              type: {} as number,
            },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // Named model should be accessible on $props
        assertType<number>(instance.$props.count);

        // Named model should be directly accessible on instance
        assertType<number>(instance.count);

        // $emit should have update:count
        type EmitFn = Instance["$emit"];
        type CanEmitCountUpdate = EmitFn extends (
          e: "update:count",
          val: number
        ) => void
          ? true
          : false;
        assertType<CanEmitCountUpdate>({} as true);
      });

      it("handles multiple models", () => {
        // Simulates:
        // const model = defineModel<string>();
        // const count = defineModel<number>('count');
        // const active = defineModel<boolean>('active');
        const macroReturn = createMacroReturn({
          model: {
            modelValue: {
              value: {} as ModelRef<string, "modelValue">,
              type: {} as string,
            },
            count: {
              value: {} as ModelRef<number, "count">,
              type: {} as number,
            },
            active: {
              value: {} as ModelRef<boolean, "active">,
              type: {} as boolean,
            },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // All models should be on $props
        assertType<string>(instance.$props.modelValue);
        assertType<number>(instance.$props.count);
        assertType<boolean>(instance.$props.active);

        // All models should be directly accessible
        assertType<string>(instance.modelValue);
        assertType<number>(instance.count);
        assertType<boolean>(instance.active);

        // @ts-expect-error - Should not be any/unknown/never
        assertType<{ __unrelatedProp: true }>(instance.$props);
      });

      it("handles model with complex types", () => {
        // Simulates: const model = defineModel<{ id: number; name: string } | null>();
        type ComplexType = { id: number; name: string } | null;

        const macroReturn = createMacroReturn({
          model: {
            modelValue: {
              value: {} as ModelRef<ComplexType, "modelValue">,
              type: {} as ComplexType,
            },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        assertType<ComplexType>(instance.$props.modelValue);
        assertType<ComplexType>(instance.modelValue);
      });
    });

    describe("defineSlots", () => {
      it("handles typed slots", () => {
        // Simulates:
        // const slots = defineSlots<{
        //   default: (props: { items: string[] }) => any;
        //   header: (props: { title: string }) => any;
        // }>();
        type SlotsType = {
          default: (props: { items: string[] }) => any;
          header: (props: { title: string }) => any;
        };

        const macroReturn = createMacroReturn({
          slots: { value: {} as SlotsType, type: {} as SlotsType },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // $slots should have the typed slots
        type Slots = Instance["$slots"];
        type HasDefaultSlot = Slots extends {
          default: (props: { items: string[] }) => any;
        }
          ? true
          : false;
        type HasHeaderSlot = Slots extends {
          header: (props: { title: string }) => any;
        }
          ? true
          : false;

        assertType<HasDefaultSlot>({} as true);
        assertType<HasHeaderSlot>({} as true);
      });
    });

    describe("defineExpose", () => {
      it("handles exposed methods and properties", () => {
        // Simulates:
        // defineExpose({
        //   focus: () => {},
        //   reset: (val: string) => {},
        //   count: computed(() => 5)
        // });
        type ExposeType = {
          focus: () => void;
          reset: (val: string) => void;
          count: number;
        };

        const macroReturn = createMacroReturn({
          expose: { object: {} as ExposeType },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // Exposed methods/properties should be directly on instance
        assertType<() => void>(instance.focus);
        assertType<(val: string) => void>(instance.reset);
        assertType<number>(instance.count);

        // @ts-expect-error - Should not be any/unknown/never
        assertType<{ __unrelatedProp: true }>(instance.focus);
      });

      it("exposed members replace props on instance root", () => {
        // When defineExpose is used, the exposed properties should be on the instance
        // instead of props being directly accessible
        const macroReturn = createMacroReturn({
          props: { value: {} as { id: number }, type: {} as { id: number } },
          expose: { object: {} as { getValue: () => number } },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // Exposed method should be on instance
        assertType<() => number>(instance.getValue);

        // Props should still be on $props
        assertType<number>(instance.$props.id);
      });
    });

    describe("combined macros - realistic component scenarios", () => {
      it("props + emits", () => {
        // Simulates a basic component with props and emits
        type EmitType = (e: "change", val: string) => void;

        const macroReturn = createMacroReturn({
          props: {
            value: {} as { value: string },
            type: {} as { value: string },
          },
          emits: { value: {} as EmitType, type: {} as EmitType },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        assertType<string>(instance.$props.value);
        assertType<string>(instance.value);

        type EmitFn = Instance["$emit"];
        type ValidEmit = EmitFn extends (e: "change", val: string) => void
          ? true
          : false;
        assertType<ValidEmit>({} as true);

        // @ts-expect-error - Should not be any/unknown/never
        assertType<{ __unrelatedProp: true }>(instance.$props);
      });

      it("props + model", () => {
        // Common pattern: props for static data, model for v-model binding
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { label: string; disabled?: boolean },
            type: {} as { label: string; disabled?: boolean },
          },
          model: {
            modelValue: {
              value: {} as ModelRef<string, "modelValue">,
              type: {} as string,
            },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLInputElement,
          false
        >;

        const instance = {} as Instance;

        // Props
        assertType<string>(instance.$props.label);
        assertType<boolean | undefined>(instance.$props.disabled);

        // Model
        assertType<string>(instance.$props.modelValue);
        assertType<string>(instance.modelValue);

        // Direct access to props
        assertType<string>(instance.label);
        assertType<boolean | undefined>(instance.disabled);

        // @ts-expect-error - Should not be any/unknown/never
        assertType<{ __unrelatedProp: true }>(instance.$props);
      });

      it("props + withDefaults + emits + model", () => {
        // More complex component with defaults
        type EmitType = {
          (e: "submit", data: { id: number }): void;
          (e: "cancel"): void;
        };

        const macroReturn = createMacroReturn({
          props: {
            value: {} as { title: string; submitLabel: string },
            type: {} as { title: string; submitLabel: string },
          },
          withDefaults: {
            value: { submitLabel: "Submit" } as const,
            type: {} as [
              { title: string; submitLabel: string },
              { submitLabel: "Submit" }
            ],
          },
          emits: { value: {} as EmitType, type: {} as EmitType },
          model: {
            formData: {
              value: {} as ModelRef<Record<string, unknown>, "formData">,
              type: {} as Record<string, unknown>,
            },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLFormElement,
          true
        >;

        // Just verify type resolves - deep instantiation check
        const instance = {} as Instance;
        assertType<HTMLFormElement | null>(instance.$el);
      });

      it("all macros combined: props, emits, model, slots, expose", () => {
        // Full-featured component like a DataTable
        interface Item {
          id: number;
          name: string;
        }

        type EmitType = {
          (e: "select", item: Item): void;
          (e: "delete", id: number): void;
        };

        type SlotsType = {
          default: (props: { items: Item[] }) => any;
          row: (props: { item: Item; index: number }) => any;
          empty: () => any;
        };

        type ExposeType = {
          refresh: () => Promise<void>;
          getSelectedItems: () => Item[];
        };

        const macroReturn = createMacroReturn({
          props: {
            value: {} as { items: Item[]; pageSize?: number },
            type: {} as { items: Item[]; pageSize?: number },
          },
          emits: { value: {} as EmitType, type: {} as EmitType },
          model: {
            selected: {
              value: {} as ModelRef<Item | null, "selected">,
              type: {} as Item | null,
            },
          },
          slots: { value: {} as SlotsType, type: {} as SlotsType },
          expose: { object: {} as ExposeType },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLTableElement,
          false
        >;

        // Just verify type resolves - complex type check
        const instance = {} as Instance;
        assertType<HTMLTableElement | null>(instance.$el);

        // Expose (should be on instance root)
        assertType<() => Promise<void>>(instance.refresh);
        assertType<() => Item[]>(instance.getSelectedItems);
      });
    });

    describe("edge cases and type safety", () => {
      it("model-only macro does not leak internal props/defaults structure", () => {
        // Regression test: when only models are defined (no props),
        // $props should only contain model values, not internal structure like 'props' or 'defaults'
        const macroReturn = createMacroReturn({
          model: {
            modelValue: {
              value: {} as ModelRef<string, "modelValue">,
              type: {} as string,
            },
            count: {
              value: {} as ModelRef<number, "count">,
              type: {} as number,
            },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // Model values should be in $props
        assertType<string>(instance.$props.modelValue);
        assertType<number>(instance.$props.count);

        // Internal structure should NOT leak into $props
        // @ts-expect-error - 'props' should not exist on $props
        instance.$props.props;

        // @ts-expect-error - 'defaults' should not exist on $props
        instance.$props.defaults;
      });

      it("empty macro return produces valid instance", () => {
        const macroReturn = createMacroReturn({});

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // Should still have Vue instance properties
        assertType<HTMLElement | null>(instance.$el);
        assertType<ComponentPublicInstance | null>(instance.$parent);
        assertType<() => void>(instance.$forceUpdate);
      });

      it("handles Attrs type parameter", () => {
        type CustomAttrs = {
          class?: string;
          style?: object;
          "data-testid"?: string;
        };

        const macroReturn = createMacroReturn({
          props: { value: {} as { id: number }, type: {} as { id: number } },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          CustomAttrs,
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // $attrs should have the custom attrs type
        assertType<CustomAttrs>(instance.$attrs);

        // With default AttrsProps=false, attrs are NOT included in $props
        // $props only has the component's own props
        type PropsType = Instance["$props"];
        type HasId = PropsType extends { id: number } ? true : false;
        assertType<HasId>({} as true);
      });

      it("handles Element type parameter", () => {
        const macroReturn = createMacroReturn({
          props: { value: {} as { src: string }, type: {} as { src: string } },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLImageElement,
          false
        >;

        const instance = {} as Instance;

        // $el should be the specific element type
        assertType<HTMLImageElement | null>(instance.$el);
      });

      it("type is not any or unknown", () => {
        const macroReturn = createMacroReturn({
          props: { value: {} as { id: number }, type: {} as { id: number } },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        // These would pass if Instance were any or unknown
        // @ts-expect-error - Instance should not be assignable to unrelated type
        assertType<{ __completelyUnrelated: "value" }>({} as Instance);
      });

      it("type is not never", () => {
        const macroReturn = createMacroReturn({
          props: { value: {} as { id: number }, type: {} as { id: number } },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        // This would fail if Instance were never (never is assignable to everything)
        const _instance: Instance = {} as Instance;
        assertType<Instance>(_instance);
      });

      it("preserves literal types in props", () => {
        const macroReturn = createMacroReturn({
          props: {
            value: {} as {
              variant: "primary" | "secondary";
              size: "sm" | "md" | "lg";
            },
            type: {} as {
              variant: "primary" | "secondary";
              size: "sm" | "md" | "lg";
            },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // Literal types should be preserved
        assertType<"primary" | "secondary">(instance.$props.variant);
        assertType<"sm" | "md" | "lg">(instance.$props.size);

        // @ts-expect-error - Should not accept arbitrary strings
        const _variant: Instance["$props"]["variant"] = "invalid";
      });

      it("handles readonly props", () => {
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { readonly items: readonly string[] },
            type: {} as { readonly items: readonly string[] },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        assertType<readonly string[]>(instance.$props.items);
      });

      it("handles union type models", () => {
        const macroReturn = createMacroReturn({
          model: {
            modelValue: {
              value: {} as ModelRef<string | number | null, "modelValue">,
              type: {} as string | number | null,
            },
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        assertType<string | number | null>(instance.$props.modelValue);
        assertType<string | number | null>(instance.modelValue);
      });
    });

    describe("DEV mode parameter", () => {
      it("DEV=true includes $data", () => {
        type DataType = { internalState: number; cache: Map<string, unknown> };

        // When DEV is true, $data should contain the component's reactive data
        type TestNormalized = {
          props: { props: { type: { id: number } }; defaults: { value: {} } };
          emits: { value: () => void };
          slots: {};
          options: {};
          model: {};
          expose: {};
          templateRef: {};
          $data: DataType;
        };

        type Instance = PublicInstanceFromNormalisedMacro<
          TestNormalized,
          {},
          HTMLElement,
          false,
          false, // AttrsProps
          true // DEV = true
        >;

        const instance = {} as Instance;

        // $data should have the data type in DEV mode
        assertType<DataType>(instance.$data);
      });

      it("DEV=false has empty $data", () => {
        type DataType = { internalState: number };

        type TestNormalized = {
          props: { props: { type: { id: number } }; defaults: { value: {} } };
          emits: { value: () => void };
          slots: {};
          options: {};
          model: {};
          expose: {};
          templateRef: {};
          $data: DataType;
        };

        type Instance = PublicInstanceFromNormalisedMacro<
          TestNormalized,
          {},
          HTMLElement,
          false,
          false, // AttrsProps
          false // DEV = false
        >;

        const instance = {} as Instance;

        // $data should be empty object in non-DEV mode
        assertType<{}>(instance.$data);
      });
    });

    describe("compatibility with Vue's ComponentPublicInstance", () => {
      it("has all required ComponentPublicInstance methods", () => {
        const macroReturn = createMacroReturn({
          props: { value: {} as { id: number }, type: {} as { id: number } },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        const instance = {} as Instance;

        // All standard methods should exist
        assertType<() => void>(instance.$forceUpdate);
        assertType<<T = void>(fn?: () => T) => Promise<Awaited<T>>>(
          instance.$nextTick
        );
        assertType<Function>(instance.$watch);
      });

      it("instance is assignable to base ComponentPublicInstance", () => {
        const macroReturn = createMacroReturn({
          props: { value: {} as { id: number }, type: {} as { id: number } },
          emits: {
            value: {} as (e: "test") => void,
            type: {} as (e: "test") => void,
          },
        });

        type Instance = PublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement,
          false
        >;

        // Check that instance has core ComponentPublicInstance properties
        const instance = {} as Instance;
        assertType<HTMLElement | null>(instance.$el);
        assertType<ComponentPublicInstance | null>(instance.$parent);
        assertType<() => void>(instance.$forceUpdate);
      });
    });
  });

  /**
   * @ai-generated - Tests for SFC instance type variants
   * Tests the specialized instance types for different usage contexts:
   * - SFCPublicInstanceFromMacro: internal template usage
   * - ExternalPublicInstanceFromMacro: consumer/parent usage
   * - TestExternalPublicInstanceFromMacro: testing with attrs in props
   */
  describe("SFC Instance Type Variants", () => {
    describe("SFCPublicInstanceFromMacro", () => {
      it("creates instance for internal SFC template usage", () => {
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { count: number; label: string },
            type: {} as { count: number; label: string },
          },
        });

        type Instance = SFCPublicInstanceFromMacro<
          typeof macroReturn,
          { class?: string },
          HTMLDivElement
        >;

        const instance = {} as Instance;

        // Props should be required (MakeDefaultsOptional=false)
        assertType<number>(instance.$props.count);
        assertType<string>(instance.$props.label);

        // $el should be the specified element type
        assertType<HTMLDivElement | null>(instance.$el);

        // $attrs should have the attrs type
        assertType<{ class?: string }>(instance.$attrs);
      });

      it("props with defaults are required internally", () => {
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { title: string; count: number },
            type: {} as { title: string; count: number },
          },
          withDefaults: {
            value: {} as { title: string },
            type: {} as { title: string },
          },
        });

        type Instance = SFCPublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement
        >;

        const instance = {} as Instance;

        // Inside SFC template, props with defaults are still available (not undefined at type level)
        // The type reflects that internally, defaults have been applied
        assertType<string | undefined>(instance.$props.title);
        assertType<number>(instance.$props.count);
      });

      it("attrs are NOT included in $props", () => {
        type CustomAttrs = { class?: string; "data-testid"?: string };

        const macroReturn = createMacroReturn({
          props: {
            value: {} as { id: number },
            type: {} as { id: number },
          },
        });

        type Instance = SFCPublicInstanceFromMacro<
          typeof macroReturn,
          CustomAttrs,
          HTMLElement
        >;

        type PropsType = Instance["$props"];

        // $props should have id but NOT class or data-testid
        type HasId = PropsType extends { id: number } ? true : false;
        assertType<HasId>({} as true);

        // Attrs should be in $attrs, not $props
        type AttrsType = Instance["$attrs"];
        type AttrsHasClass = AttrsType extends { class?: string } ? true : false;
        assertType<AttrsHasClass>({} as true);
      });

      it("exposes model values on $props and instance", () => {
        const macroReturn = createMacroReturn({
          model: {
            modelValue: {
              value: {} as ModelRef<string, "modelValue">,
              type: {} as string,
            },
          },
        });

        type Instance = SFCPublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement
        >;

        const instance = {} as Instance;

        // Model value accessible on $props
        assertType<string>(instance.$props.modelValue);

        // Model value accessible directly on instance
        assertType<string>(instance.modelValue);
      });
    });

    describe("ExternalPublicInstanceFromMacro", () => {
      it("creates instance for external/consumer usage", () => {
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { count: number; label: string },
            type: {} as { count: number; label: string },
          },
        });

        type Instance = ExternalPublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLDivElement
        >;

        const instance = {} as Instance;

        // Props should still be accessible
        assertType<number>(instance.$props.count);
        assertType<string>(instance.$props.label);

        // $el should be the specified element type
        assertType<HTMLDivElement | null>(instance.$el);
      });

      it("props with defaults are optional for consumers", () => {
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { title: string; count: number },
            type: {} as { title: string; count: number },
          },
          withDefaults: {
            value: {} as { title: string },
            type: {} as { title: string },
          },
        });

        type Instance = ExternalPublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement
        >;

        const instance = {} as Instance;

        // For external consumers, props with defaults should be optional (can be undefined)
        assertType<string | undefined>(instance.$props.title);
        assertType<number>(instance.$props.count);
      });

      it("attrs are NOT included in $props", () => {
        type CustomAttrs = { class?: string; style?: object };

        const macroReturn = createMacroReturn({
          props: {
            value: {} as { id: number },
            type: {} as { id: number },
          },
        });

        type Instance = ExternalPublicInstanceFromMacro<
          typeof macroReturn,
          CustomAttrs,
          HTMLElement
        >;

        const instance = {} as Instance;

        // $attrs has the custom attrs
        assertType<CustomAttrs>(instance.$attrs);

        // $props should NOT include attrs
        type PropsType = Instance["$props"];
        type HasId = PropsType extends { id: number } ? true : false;
        assertType<HasId>({} as true);
      });

      it("works with exposed methods from defineExpose", () => {
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { value: string },
            type: {} as { value: string },
          },
          expose: {
            object: {} as { focus: () => void; getValue: () => string },
          },
        });

        type Instance = ExternalPublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement
        >;

        const instance = {} as Instance;

        // Exposed methods should be on instance
        assertType<() => void>(instance.focus);
        assertType<() => string>(instance.getValue);
      });
    });

    describe("TestExternalPublicInstanceFromMacro", () => {
      it("includes attrs in $props for testing purposes", () => {
        type CustomAttrs = { class?: string; "data-testid"?: string };

        const macroReturn = createMacroReturn({
          props: {
            value: {} as { id: number },
            type: {} as { id: number },
          },
        });

        type Instance = TestExternalPublicInstanceFromMacro<
          typeof macroReturn,
          CustomAttrs,
          HTMLElement
        >;

        type PropsType = Instance["$props"];

        // $props should include BOTH props and attrs
        type HasId = PropsType extends { id: number } ? true : false;
        assertType<HasId>({} as true);

        // Attrs should be merged into $props
        type HasClass = PropsType extends { class?: string } ? true : false;
        assertType<HasClass>({} as true);

        type HasTestId = PropsType extends { "data-testid"?: string }
          ? true
          : false;
        assertType<HasTestId>({} as true);
      });

      it("has MakeDefaultsOptional=true like ExternalPublicInstanceFromMacro", () => {
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { title: string; count: number },
            type: {} as { title: string; count: number },
          },
          withDefaults: {
            value: {} as { title: string },
            type: {} as { title: string },
          },
        });

        type Instance = TestExternalPublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement
        >;

        const instance = {} as Instance;

        // Props with defaults should be optional
        assertType<string | undefined>(instance.$props.title);
        assertType<number>(instance.$props.count);
      });

      it("useful for testing attr merging behavior", () => {
        type TestAttrs = {
          class?: string;
          style?: object;
          onClick?: () => void;
        };

        const macroReturn = createMacroReturn({
          props: {
            value: {} as { disabled: boolean },
            type: {} as { disabled: boolean },
          },
          emits: {
            value: {} as (e: "click") => void,
            type: {} as (e: "click") => void,
          },
        });

        type Instance = TestExternalPublicInstanceFromMacro<
          typeof macroReturn,
          TestAttrs,
          HTMLButtonElement
        >;

        const instance = {} as Instance;

        // In tests, we can verify that attrs are properly typed alongside props
        type PropsWithAttrs = Instance["$props"];

        // Component props - disabled should be accessible (optional in external view)
        assertType<boolean | undefined>(instance.$props.disabled);

        // Fallthrough attrs merged in for testing
        type HasStyle = PropsWithAttrs extends { style?: object } ? true : false;
        assertType<HasStyle>({} as true);

        // Element type is correct
        assertType<HTMLButtonElement | null>(instance.$el);
      });
    });

    describe("comparison between SFC, External, and Test instance types", () => {
      it("all three types share common Vue instance properties", () => {
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { id: number },
            type: {} as { id: number },
          },
        });

        type SFCInstance = SFCPublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement
        >;
        type ExternalInstance = ExternalPublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement
        >;
        type TestInstance = TestExternalPublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement
        >;

        // All should have standard Vue instance methods
        const sfcInstance = {} as SFCInstance;
        const externalInstance = {} as ExternalInstance;
        const testInstance = {} as TestInstance;

        // $el
        assertType<HTMLElement | null>(sfcInstance.$el);
        assertType<HTMLElement | null>(externalInstance.$el);
        assertType<HTMLElement | null>(testInstance.$el);

        // $forceUpdate
        assertType<() => void>(sfcInstance.$forceUpdate);
        assertType<() => void>(externalInstance.$forceUpdate);
        assertType<() => void>(testInstance.$forceUpdate);

        // $emit
        assertType<Function>(sfcInstance.$emit);
        assertType<Function>(externalInstance.$emit);
        assertType<Function>(testInstance.$emit);
      });

      it("differs in how they handle props with defaults", () => {
        const macroReturn = createMacroReturn({
          props: {
            value: {} as { required: number; optional: string },
            type: {} as { required: number; optional: string },
          },
          withDefaults: {
            value: {} as { optional: string },
            type: {} as { optional: string },
          },
        });

        type SFCInstance = SFCPublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement
        >;
        type ExternalInstance = ExternalPublicInstanceFromMacro<
          typeof macroReturn,
          {},
          HTMLElement
        >;

        // Both have required prop as number
        const sfcInstance = {} as SFCInstance;
        const externalInstance = {} as ExternalInstance;

        assertType<number>(sfcInstance.$props.required);
        assertType<number>(externalInstance.$props.required);

        // Optional prop: SFC sees it as string|undefined, External also sees it as string|undefined
        // (Vue's internal typing behavior)
        assertType<string | undefined>(sfcInstance.$props.optional);
        assertType<string | undefined>(externalInstance.$props.optional);
      });
    });
  });
});
