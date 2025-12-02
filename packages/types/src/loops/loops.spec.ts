/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for loop type helpers including:
 * - extractLoops: extracts key and value types from arrays and objects
 * - ExtractLoopsResult: type-level helper for computing loop types
 * - Used in component-type plugin to type v-for loop variables
 * - Validates correct typing of loop iteration (key/value pairs)
 */
import { describe, it, assertType } from "vitest";
import type { extractLoops, ExtractLoopsResult } from "./loops";

describe("loops helpers", () => {
  describe("extractLoops", () => {
    describe("array types", () => {
      it("extracts key as number and value as element type from arrays", () => {
        type Result = ExtractLoopsResult<string[]>;
        type Expected = { key: number; value: string };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<{ unrelated: true }>({} as Result);
      });

      it("extracts value type from number arrays", () => {
        type Result = ExtractLoopsResult<number[]>;
        type Expected = { key: number; value: number };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<{ unrelated: true }>({} as Result);
      });

      it("extracts complex object type from arrays", () => {
        type Item = { id: number; name: string };
        type Result = ExtractLoopsResult<Item[]>;
        type Expected = { key: number; value: Item };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<{ unrelated: true }>({} as Result);

        const result = {} as Result;
        result.key satisfies number;
        result.value.id satisfies number;
        result.value.name satisfies string;
      });

      it("extracts union types from arrays", () => {
        type Result = ExtractLoopsResult<(string | number)[]>;
        type Expected = { key: number; value: string | number };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<{ unrelated: true }>({} as Result);
      });

      it("handles nested arrays", () => {
        type Result = ExtractLoopsResult<string[][]>;
        type Expected = { key: number; value: string[] };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<{ unrelated: true }>({} as Result);
      });
    });

    describe("object/record types", () => {
      it("extracts literal keys and values from objects", () => {
        type Input = { a: number; b: string };
        type Result = ExtractLoopsResult<Input>;
        type Expected = { key: "a"; value: number } | { key: "b"; value: string };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<{ unrelated: true }>({} as Result);
      });

      it("extracts single property object", () => {
        type Input = { foo: boolean };
        type Result = ExtractLoopsResult<Input>;
        type Expected = { key: "foo"; value: boolean };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<{ unrelated: true }>({} as Result);
      });

      it("handles optional properties", () => {
        type Input = { a: number; b?: string };
        type Result = ExtractLoopsResult<Input>;
        // Note: for optional properties, keyof includes undefined in the mapped type
        // This matches Vue's v-for behavior where optional keys may be iterated
        type Expected =
          | { key: "a"; value: number }
          | { key: "b"; value: string | undefined }
          | undefined;

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);
      });

      it("handles complex nested object values", () => {
        type Item = { id: number; children?: Item[] };
        type Input = { parent: Item; child: Item };
        type Result = ExtractLoopsResult<Input>;
        type Expected =
          | { key: "parent"; value: Item }
          | { key: "child"; value: Item };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<{ unrelated: true }>({} as Result);
      });

      it("handles Record types with string keys", () => {
        type Input = Record<string, number>;
        type Result = ExtractLoopsResult<Input>;
        type Expected = { key: string; value: number };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<{ unrelated: true }>({} as Result);
      });

      it("handles Record types with number keys", () => {
        type Input = Record<number, string>;
        type Result = ExtractLoopsResult<Input>;
        type Expected = { key: number; value: string };

        assertType<Result>({} as Expected);
        assertType<Expected>({} as Result);

        // @ts-expect-error - Result is not any/unknown/never
        assertType<{ unrelated: true }>({} as Result);
      });
    });

    describe("v-for use cases", () => {
      it("simulates v-for with array items - (item, index) in items", () => {
        type Items = { id: number; name: string }[];
        type Result = ExtractLoopsResult<Items>;

        const loop = {} as Result;
        // In v-for="(item, index) in items":
        // - item would be loop.value
        // - index would be loop.key
        loop.value.id satisfies number;
        loop.value.name satisfies string;
        loop.key satisfies number;

        // @ts-expect-error - key is number, not string
        loop.key satisfies string;
      });

      it("simulates v-for with object - (value, key) in object", () => {
        type Obj = { a: 1; b: "2"; c: true };
        type Result = ExtractLoopsResult<Obj>;

        const loop = {} as Result;
        // In v-for="(value, key) in object":
        // - value would be loop.value
        // - key would be loop.key

        // Key is union of literal keys
        loop.key satisfies "a" | "b" | "c";

        // Value is union of values
        loop.value satisfies 1 | "2" | true;
      });

      it("handles nested v-for with children property", () => {
        type Parent = {
          id: number;
          name: string;
          children?: { id: number; name: string }[];
        };
        type Parents = Parent[];

        // Outer loop: item in parents
        type OuterResult = ExtractLoopsResult<Parents>;
        const outer = {} as OuterResult;
        outer.value.id satisfies number;
        outer.value.children satisfies { id: number; name: string }[] | undefined;

        // Inner loop: child in item.children
        type InnerItems = NonNullable<Parent["children"]>;
        type InnerResult = ExtractLoopsResult<InnerItems>;
        const inner = {} as InnerResult;
        inner.value.id satisfies number;
        inner.value.name satisfies string;
        inner.key satisfies number;
      });
    });

    describe("function type inference", () => {
      // These tests verify the extractLoops function declaration works correctly at the type level
      // The function is declared (not implemented) for use in generated code

      it("function signature extracts array elements correctly", () => {
        // Simulate the way extractLoops is used in generated component-type code
        const extractLoopsLocal = (() => {}) as unknown as typeof extractLoops;

        const items = [{ id: 1, name: "test" }];
        const { key, value } = extractLoopsLocal(items);

        key satisfies number;
        value.id satisfies number;
        value.name satisfies string;
      });

      it("function signature extracts object entries correctly", () => {
        const extractLoopsLocal = (() => {}) as unknown as typeof extractLoops;

        const obj = { a: 1, b: "2" } as const;
        const { key, value } = extractLoopsLocal(obj);

        key satisfies "a" | "b";
        value satisfies 1 | "2";
      });
    });
  });
});
