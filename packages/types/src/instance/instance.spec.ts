/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for Vue component instance type helpers including:
 * - ToInstanceProps, CreateTypedPublicInstanceFromNormalisedMacro
 * - InternalInstanceFromMacro, PublicInstanceFromNormalisedMacro
 * - CreateExportedInstanceFromNormalisedMacro, CreateExportedInstanceFromMacro
 * - Verifies props/models are accessible on instances and ComponentPublicInstance compatibility
 */
import { describe, it, assertType } from "vitest";
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
} from "./instance";
import { createMacroReturn } from "../setup";
import type { PropsWithDefaults } from "../props";

describe("instance helpers", () => {
  describe("ToInstanceProps", () => {
    it("extracts props with MakePublicProps when MakeDefaultsOptional is true", () => {
      type TestProps = PropsWithDefaults<{ id: number; name: string }, "name">;
      type Props = ToInstanceProps<TestProps, true>;

      // name has default, so should be optional
      const props: Props = { id: 1, name: undefined };
      assertType<number>(props.id);
      assertType<string | undefined>(props.name);
    });

    it("extracts props with MakeInternalProps when MakeDefaultsOptional is false", () => {
      type TestProps = PropsWithDefaults<{ id: number; name: string }, "name">;
      type Props = ToInstanceProps<TestProps, false>;

      // internally, name with default is required (always defined)
      const props: Props = { id: 1, name: "test" };
      assertType<number>(props.id);
      assertType<string | undefined>(props.name);
    });

    it("handles empty props object", () => {
      type Props = ToInstanceProps<{}, true>;
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
        [k: symbol]: {
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
          value: {
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
      assertType<number>(instance.id);
    });

    it("exported instance exposes methods from defineExpose", () => {
      type TestNormalized = {
        props: { props: { type: { name: string } }; defaults: { value: {} } };
        emits: { value: (e: "change", val: string) => void };
        slots: {};
        options: {};
        model: {};
        expose: {
          value: {
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
      assertType<string | undefined>(instance.name);

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
          value: {
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
          value: {
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
      assertType<string>(instance.label);
      assertType<boolean>(instance.disabled);

      // Model accessible
      assertType<string>(instance.modelValue);

      // Exposed methods accessible
      assertType<() => void>(instance.submit);
      assertType<() => void>(instance.reset);
      assertType<() => boolean>(instance.isValid);

      // $el is the element type
      assertType<HTMLFormElement | null>(instance.$el);
    });

    it("CreateExportedInstanceFromMacro with expose", () => {
      type MacroReturnType = {
        [k: symbol]: {
          props: { value: { id: number }; type: { id: number } };
          expose: {
            value: {
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
          value: {
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
});
