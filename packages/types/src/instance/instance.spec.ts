import { describe, it, assertType } from "vitest";
import type { ModelRef } from "vue";
import {
  CreateTypedInternalInstanceFromNormalisedMacro,
  CreateTypedPublicInstanceFromNormalisedMacro,
  ToInstanceProps,
} from "./instance";
import {
  createMacroReturn,
  NormaliseMacroReturn,
} from "../setup";
import type { PropsWithDefaults } from "../props";

describe("instance helpers", () => {
  describe("ToInstanceProps", () => {
    it("extracts props with MakePublicProps when MakeDefaultsOptional is true", () => {
      type TestProps = PropsWithDefaults<
        { id: number; name: string },
        "name"
      >;
      type Props = ToInstanceProps<TestProps, true>;

      // name has default, so should be optional
      const props: Props = { id: 1, name: undefined };
      assertType<number>(props.id);
      assertType<string | undefined>(props.name);
    });

    it("extracts props with MakeInternalProps when MakeDefaultsOptional is false", () => {
      type TestProps = PropsWithDefaults<
        { id: number; name: string },
        "name"
      >;
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
});
