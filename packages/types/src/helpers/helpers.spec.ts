import { describe, it, assertType } from "vitest";
import {
  ExtractHidden,
  PatchHidden,
  FunctionToObject,
  IntersectionFunctionToObject,
  UnionToIntersection,
  PartialUndefined,
} from "./helpers";

describe("Helpers", () => {
  describe("PatchHidden and ExtractHidden", () => {
    it("should patch and extract hidden properties correctly", () => {
      type OriginalType = { a: number; b: string };
      type HiddenType = { hiddenProp: boolean };
      type PatchedType = PatchHidden<OriginalType, HiddenType>;

      type ExtractedType = ExtractHidden<PatchedType>;

      // Assert that the patched type still has the original properties
      assertType<PatchedType>({} as OriginalType);
      assertType<OriginalType>({} as PatchedType);
      assertType<HiddenType>({} as ExtractedType);

      assertType<PatchedType>({ a: 42, b: "hello" });
      assertType<ExtractedType>({ hiddenProp: true });
    });

    it("should be never when extracting from a type without the hidden property", () => {
      type SomeType = { x: number; y: string };
      type ExtractedType = ExtractHidden<SomeType>;
      assertType<ExtractedType>({} as never);
    });
  });

  describe("FunctionToObject & IntersectionFunctionToObject", () => {
    it("simple", () => {
      type Fn = (e: "foo", a: number) => void;
      type Obj = FunctionToObject<Fn>;
      type ExpectedObj = {
        foo: [a: number];
      };
      type Extracted = ExtractHidden<Obj>;

      assertType<Fn>({} as Obj);
      assertType<Extracted>({} as ExpectedObj);

      // @ts-expect-error test for any type
      assertType<{ a: 1 }>({} as Extracted);
      // @ts-expect-error test for any type
      assertType<{ a: 1 }>({} as Obj);

      const test = {} as Extracted;
      test.foo;
      <number>test.foo[0];
      //@ts-expect-error non-existent event
      test.a;
      //@ts-expect-error wrong arg type
      <string>test.foo[0];
    });

    it("no args", () => {
      type Fn = (e: "foo") => void;
      type Obj = FunctionToObject<Fn>;
      type ExpectedObj = {
        foo: [];
      };
      type Extracted = ExtractHidden<Obj>;

      assertType<Fn>({} as Obj);
      assertType<Extracted>({} as ExpectedObj);

      // @ts-expect-error test for any type
      assertType<{ a: 1 }>({} as Extracted);
      // @ts-expect-error test for any type
      assertType<{ a: 1 }>({} as Obj);

      const test = {} as Extracted;
      test.foo;
      //@ts-expect-error wrong number of args
      test.foo[0];
      //@ts-expect-error non-existent event
      test.a;
      //@ts-expect-error wrong arg type
      <number>test.foo[0];
    });

    it("intersections", () => {
      type Fn = ((e: "foo", a: number) => void) &
        ((e: "bar", b: string) => void) &
        ((e: "baz") => void);

      type Obj = IntersectionFunctionToObject<Fn>;
      type ExpectedObj = {
        foo: [a: number];
        bar: [b: string];
        baz: [];
      };
      type Extracted = ExtractHidden<Obj>;

      assertType<Fn>({} as Obj);
      assertType<Extracted>({} as ExpectedObj);

      // @ts-expect-error test for any type
      assertType<{ a: 1 }>({} as Extracted);
      // @ts-expect-error test for any type
      assertType<{ a: 1 }>({} as Obj);

      const test = {} as Extracted;
      test.foo;
      <number>test.foo[0];
      test.bar;
      <string>test.bar[0];
      test.baz;
      //@ts-expect-error wrong number of args
      test.baz[0];
      //@ts-expect-error non-existent event
      test.a;
      //@ts-expect-error wrong arg type
      <string>test.foo[0];
    });

    it("union functions", () => {
      type Fn =
        | ((e: "foo", a: number) => void)
        | ((e: "bar", b: string) => void)
        | ((e: "baz") => void);

      type Obj = IntersectionFunctionToObject<Fn>;
      type ExpectedObj = {
        foo: [a: number];
        bar: [b: string];
        baz: [];
      };
      // since the types are unioned, we need to convert to intersection
      type Extracted = UnionToIntersection<ExtractHidden<Obj>>;

      assertType<Fn>({} as Obj);
      assertType<Extracted>({} as ExpectedObj);

      // @ts-expect-error test for any type
      assertType<{ a: 1 }>({} as Extracted);
      // @ts-expect-error test for any type
      assertType<{ a: 1 }>({} as Obj);

      const test = {} as Extracted;
      test.foo;
      <number>test.foo[0];
      test.bar;
      <string>test.bar[0];
      test.baz;
      //@ts-expect-error wrong number of args
      test.baz[0];
      //@ts-expect-error non-existent event
      test.a;
      //@ts-expect-error wrong arg type
      <string>test.foo[0];
    });

    it('arg types "foo" | "bar"', () => {
      type Fn = (e: "foo" | "bar", a: number) => void;

      type Obj = IntersectionFunctionToObject<Fn>;
      type ExpectedObj = {
        foo: [a: number];
        bar: [a: number];
      };
      // since the types are unioned, we need to convert to intersection
      type Extracted = UnionToIntersection<ExtractHidden<Obj>>;

      assertType<Fn>({} as Obj);
      assertType<Extracted>({} as ExpectedObj);

      // @ts-expect-error test for any type
      assertType<{ a: 1 }>({} as Extracted);
      // @ts-expect-error test for any type
      assertType<{ a: 1 }>({} as Obj);

      const test = {} as Extracted;
      test.foo;
      <number>test.foo[0];
      test.bar;
      <number>test.bar[0];
      //@ts-expect-error wrong number of args
      test.bar[1];
      //@ts-expect-error non-existent event
      test.a;
      //@ts-expect-error wrong arg type
      <string>test.foo[0];
      //@ts-expect-error wrong arg type
      <string>test.bar[0];
    });
  });

  describe("PartialUndefined", () => {
    it("makes undefined properties optional", () => {
      type Original = {
        required: string;
        optional: number | undefined;
        alsoOptional: boolean | undefined;
      };

      type Result = PartialUndefined<Original>;
      type Expected = {
        required: string;
        optional?: number | undefined;
        alsoOptional?: boolean | undefined;
      };

      assertType<Result>({} as Expected);

      const value: Result = {
        required: "test",
      };

      value.required;
      value.optional;
      value.alsoOptional;

      //@ts-expect-error required property cannot be undefined
      const invalid: Result = {
        optional: 123,
      };
    });

    it("keeps all properties required when none accept undefined", () => {
      type Original = {
        a: string;
        b: number;
        c: boolean;
      };

      type Result = PartialUndefined<Original>;

      assertType<Result>({} as Original);

      //@ts-expect-error all properties required
      const invalid: Result = {
        a: "test",
        b: 123,
      };

      const valid: Result = {
        a: "test",
        b: 123,
        c: true,
      };
    });

    it("handles mix of required and optional properties", () => {
      type Original = {
        id: number;
        name: string;
        email: string | undefined;
        age: number | undefined;
        active: boolean;
      };

      type Result = PartialUndefined<Original>;
      type Expected = {
        id: number;
        name: string;
        active: boolean;
        email?: string | undefined;
        age?: number | undefined;
      };

      assertType<Result>({} as Expected);

      const value: Result = {
        id: 1,
        name: "John",
        active: true,
      };

      value.email;
      value.age;

      //@ts-expect-error missing required properties
      const invalid: Result = {
        email: "test@example.com",
      };
    });

    it("handles nullable properties (null | undefined)", () => {
      type Original = {
        a: string;
        b: number | null | undefined;
        c: boolean | undefined;
      };

      type Result = PartialUndefined<Original>;
      type Expected = {
        a: string;
        b?: number | null | undefined;
        c?: boolean | undefined;
      };

      assertType<Result>({} as Expected);

      const value: Result = {
        a: "test",
      };

      const valueWithNull: Result = {
        a: "test",
        b: null,
      };

      const valueWithValue: Result = {
        a: "test",
        b: 123,
        c: true,
      };
    });

    it("handles empty object", () => {
      type Original = {};
      type Result = PartialUndefined<Original>;

      assertType<Result>({} as Original);

      const value: Result = {};
    });

    it("handles object with only optional properties", () => {
      type Original = {
        a: string | undefined;
        b: number | undefined;
        c: boolean | undefined;
      };

      type Result = PartialUndefined<Original>;
      type Expected = {
        a?: string | undefined;
        b?: number | undefined;
        c?: boolean | undefined;
      };

      assertType<Result>({} as Expected);

      const value1: Result = {};
      const value2: Result = { a: "test" };
      const value3: Result = { a: "test", b: 123, c: true };
    });

    describe("generic tests", () => {
      it("preserves generics with required properties", () => {
        function makePartial<T extends { id: number; name: string }>() {
          type Result = PartialUndefined<T>;
          return {} as Result;
        }

        const result = makePartial<{ id: number; name: string }>();
        type Result = typeof result;

        assertType<Result>({} as { id: number; name: string });

        result.id;
        result.name;
      });

      it("preserves generics with optional properties", () => {
        function makePartial<T extends { id: number; value?: string }>() {
          type WithUndefined = T & { value: string | undefined };
          type Result = PartialUndefined<WithUndefined>;
          return {} as Result;
        }

        interface MyType {
          id: number;
          value?: string;
        }

        const result = makePartial<MyType>();
        type Result = typeof result;

        result.id;
        result.value;

        //@ts-expect-error id is required
        const invalid: Result = { value: "test" };
      });

      it("works with constrained generics", () => {
        function processType<T extends Record<string, any>>() {
          type WithOptional = {
            [K in keyof T]: T[K] | undefined;
          };
          type Result = PartialUndefined<WithOptional>;
          return {} as Result;
        }

        interface User {
          id: number;
          name: string;
          email: string;
        }

        const result = processType<User>();
        type Result = typeof result;

        type Expected = {
          id?: number | undefined;
          name?: string | undefined;
          email?: string | undefined;
        };

        assertType<Result>({} as Expected);

        const value1: Result = {};
        const value2: Result = { id: 1 };
        const value3: Result = { id: 1, name: "John", email: "john@test.com" };
      });

      it("handles generic union types", () => {
        function makePartial<T>() {
          type WithUndefined = {
            value: T | undefined;
            required: T;
          };
          type Result = PartialUndefined<WithUndefined>;
          return {} as Result;
        }

        const stringResult = makePartial<string>();
        type StringResult = typeof stringResult;

        type ExpectedString = {
          required: string;
          value?: string | undefined;
        };

        assertType<StringResult>({} as ExpectedString);

        stringResult.required;
        stringResult.value;

        const numberResult = makePartial<number>();
        type NumberResult = typeof numberResult;

        type ExpectedNumber = {
          required: number;
          value?: number | undefined;
        };

        assertType<NumberResult>({} as ExpectedNumber);

        numberResult.required;
        numberResult.value;
      });

      it("preserves complex generic structures", () => {
        function makePartial<T extends { data: any }>() {
          type WithUndefined = {
            id: number;
            data: T["data"];
            metadata: T["data"] | undefined;
          };
          type Result = PartialUndefined<WithUndefined>;
          return {} as Result;
        }

        interface MyData {
          data: {
            items: string[];
            count: number;
          };
        }

        const result = makePartial<MyData>();
        type Result = typeof result;

        type Expected = {
          id: number;
          data: { items: string[]; count: number };
          metadata?: { items: string[]; count: number } | undefined;
        };

        assertType<Result>({} as Expected);

        result.id;
        result.data;
        result.metadata;

        //@ts-expect-error missing required properties
        const invalid: Result = {
          metadata: { items: [], count: 0 },
        };

        const valid: Result = {
          id: 1,
          data: { items: [], count: 0 },
        };
      });
    });
  });

  describe("UnionToIntersection", () => {
    it("converts union to intersection", () => {
      type Union = { a: number } | { b: string } | { c: boolean };
      type Result = UnionToIntersection<Union>;
      type Expected = { a: number } & { b: string } & { c: boolean };

      assertType<Result>({} as Expected);

      const value: Result = {
        a: 123,
        b: "test",
        c: true,
      };

      value.a;
      value.b;
      value.c;

      //@ts-expect-error missing properties
      const invalid: Result = {
        a: 123,
        b: "test",
      };
    });

    it("handles function unions", () => {
      type Union = ((a: number) => void) | ((b: string) => void);
      type Result = UnionToIntersection<Union>;
      type Expected = ((a: number) => void) & ((b: string) => void);

      assertType<Result>({} as Expected);

      const fn: Result = ((x: any) => {}) as any;
      fn(123);
      fn("test");
    });

    it("handles empty union (never)", () => {
      type Union = never;
      type Result = UnionToIntersection<Union>;

      assertType<Result>({} as never);
    });

    describe("generic tests", () => {
      it("preserves generics in union to intersection", () => {
        function unionToIntersection<T>() {
          type Result = UnionToIntersection<T>;
          return {} as Result;
        }

        type MyUnion = { a: string } | { b: number };
        const result = unionToIntersection<MyUnion>();
        type Result = typeof result;

        type Expected = { a: string } & { b: number };

        assertType<Result>({} as Expected);

        result.a;
        result.b;
      });

      it("works with constrained generic unions", () => {
        function convert<T extends { id: number } | { name: string }>() {
          type Result = UnionToIntersection<T>;
          return {} as Result;
        }

        const result = convert<{ id: number } | { name: string }>();
        type Result = typeof result;

        type Expected = { id: number } & { name: string };

        assertType<Result>({} as Expected);

        result.id;
        result.name;
      });
    });
  });
});
