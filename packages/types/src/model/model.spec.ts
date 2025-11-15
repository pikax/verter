import { describe, it, assertType } from "vitest";
import type { ModelRef } from "vue";
import type { ModelToProps, ModelToEmits } from "./model";

describe("Model helpers", () => {
  describe("ModelToProps", () => {
    it("should convert ModelRef properties to props", () => {
      type Model = {
        title: ModelRef<string>;
        count: ModelRef<number>;
      };

      type Props = ModelToProps<Model>;
      type Expected = {
        title: string;
        count: number;
      };

      assertType<Props>({} as Expected);
      assertType<Expected>({} as Props);

      assertType<Props>({ title: "hello", count: 42 });

      // @ts-expect-error wrong type
      assertType<Props>({ title: 123, count: 42 });
      // @ts-expect-error missing property
      assertType<Props>({ title: "hello" });
    });

    it("should handle optional ModelRef properties", () => {
      type Model = {
        title: ModelRef<string> | undefined;
        count: ModelRef<number>;
      };

      type Props = ModelToProps<Model>;
      type Expected = {
        title: string | undefined;
        count: number;
      };

      assertType<Props>({} as Expected);

      assertType<Props>({ title: "hello", count: 42 });
      assertType<Props>({ title: undefined, count: 42 });

      // @ts-expect-error wrong type
      assertType<Props>({ title: 123, count: 42 });
    });

    it("should handle complex types in ModelRef", () => {
      type Model = {
        user: ModelRef<{ name: string; age: number }>;
        tags: ModelRef<string[]>;
      };

      type Props = ModelToProps<Model>;
      type Expected = {
        user: { name: string; age: number };
        tags: string[];
      };

      assertType<Props>({} as Expected);
      assertType<Expected>({} as Props);

      assertType<Props>({
        user: { name: "John", age: 30 },
        tags: ["vue", "typescript"],
      });

      // @ts-expect-error wrong nested type
      assertType<Props>({ user: { name: 123, age: 30 }, tags: [] });
      // @ts-expect-error wrong array type
      assertType<Props>({ user: { name: "John", age: 30 }, tags: [123] });
    });

    it("should handle union types in ModelRef", () => {
      type Model = {
        value: ModelRef<string | number>;
      };

      type Props = ModelToProps<Model>;
      type Expected = {
        value: string | number;
      };

      assertType<Props>({} as Expected);
      assertType<Expected>({} as Props);

      assertType<Props>({ value: "hello" });
      assertType<Props>({ value: 42 });

      // @ts-expect-error wrong type
      assertType<Props>({ value: true });
    });

    it("should filter out non-ModelRef properties", () => {
      type Model = {
        title: string;
        count: number;
      };

      type Props = ModelToProps<Model>;
      type Expected = {};

      assertType<Props>({} as Expected);
      assertType<Expected>({} as Props);

      assertType<Props>({});
    });

    it("should handle mixed ModelRef and non-ModelRef properties", () => {
      type Model = {
        title: ModelRef<string>;
        description: string;
        count: ModelRef<number>;
      };

      type Props = ModelToProps<Model>;
      type Expected = {
        title: string;
        count: number;
      };

      assertType<Props>({} as Expected);
      assertType<Expected>({} as Props);

      assertType<Props>({
        title: "hello",
        count: 42,
      });
    });

    it("should handle empty model", () => {
      type Model = {};

      type Props = ModelToProps<Model>;
      type Expected = {};

      assertType<Props>({} as Expected);
      assertType<Expected>({} as Props);

      assertType<Props>({});
    });

    it("should respect custom name in ModelRef", () => {
      type Model = {
        internalValue: ModelRef<string, "value">;
        internalCount: ModelRef<number, "count">;
      };

      type Props = ModelToProps<Model>;
      type Expected = {
        value: string;
        count: number;
      };

      assertType<Props>({} as Expected);
      assertType<Expected>({} as Props);

      assertType<Props>({ value: "hello", count: 42 });

      // @ts-expect-error original property name should not exist
      assertType<Props>({ internalValue: "hello", internalCount: 42 });
    });

    it("should use default name when ModelRef name parameter is not provided", () => {
      type Model = {
        title: ModelRef<string>;
      };

      type Props = ModelToProps<Model>;
      type Expected = {
        title: string;
      };

      assertType<Props>({} as Expected);
      assertType<Expected>({} as Props);
    });

    it("should handle mix of custom and default names", () => {
      type Model = {
        internalTitle: ModelRef<string, "title">;
        count: ModelRef<number>;
        internalActive: ModelRef<boolean, "isActive">;
      };

      type Props = ModelToProps<Model>;
      type Expected = {
        title: string;
        count: number;
        isActive: boolean;
      };

      assertType<Props>({} as Expected);
      assertType<Expected>({} as Props);

      assertType<Props>({ title: "hello", count: 42, isActive: true });
    });

    it("should handle custom names with optional ModelRef", () => {
      // Note: ModelRef<T, Name> | undefined currently doesn't preserve the name remapping
      // This is a TypeScript limitation with conditional types in unions
      type Model = {
        internalValue: ModelRef<string, "value"> | undefined;
      };

      type Props = ModelToProps<Model>;
      // The union with undefined prevents name extraction, so it uses the property name
      type Expected = {
        internalValue: string | undefined;
      };

      assertType<Props>({} as Expected);
      assertType<Expected>({} as Props);

      assertType<Props>({ internalValue: "hello" });
      assertType<Props>({ internalValue: undefined });
    });

    it("should handle custom names with complex types", () => {
      type Model = {
        internalUser: ModelRef<{ name: string; age: number }, "user">;
        internalTags: ModelRef<string[], "tags">;
      };

      type Props = ModelToProps<Model>;
      type Expected = {
        user: { name: string; age: number };
        tags: string[];
      };

      assertType<Props>({} as Expected);
      assertType<Expected>({} as Props);

      assertType<Props>({
        user: { name: "John", age: 30 },
        tags: ["vue", "typescript"],
      });
    });

    it("should handle optional props with value union pattern", () => {
      // For optional props, use the pattern where the ModelRef itself is required
      // but the value can be undefined
      type Model = {
        internalValue: ModelRef<string | undefined, "value">;
      };

      type Props = ModelToProps<Model>;
      type Expected = {
        value: string | undefined;
      };

      assertType<Props>({} as Expected);
      assertType<Expected>({} as Props);

      assertType<Props>({ value: "hello" });
      assertType<Props>({ value: undefined });
    });
  });

  describe("ModelToEmits", () => {
    it("should generate emit functions for ModelRef properties", () => {
      type Model = {
        title: ModelRef<string>;
        count: ModelRef<number>;
      };

      type Emits = ModelToEmits<Model>;

      const emits = {} as Emits;

      // These should be valid calls
      emits("update:title", "hello");
      emits("update:count", 42);
    });

    it("should handle optional ModelRef properties", () => {
      type Model = {
        title: ModelRef<string> | undefined;
        count: ModelRef<number>;
      };

      type Emits = ModelToEmits<Model>;

      const emit: Emits = {} as any;

      emit("update:title", "hello");
      emit("update:title", undefined);
      emit("update:count", 42);

      // @ts-expect-error count doesn't accept undefined
      emit("update:count", undefined);
      // @ts-expect-error wrong type
      emit("update:title", 123);
    });

    it("should handle complex types in ModelRef", () => {
      type Model = {
        user: ModelRef<{ name: string; age: number }>;
        tags: ModelRef<string[]>;
      };

      type Emits = ModelToEmits<Model>;

      const emit: Emits = {} as any;

      emit("update:user", { name: "John", age: 30 });
      emit("update:tags", ["vue", "typescript"]);

      // @ts-expect-error wrong nested type
      emit("update:user", { name: 123, age: 30 });
      // @ts-expect-error wrong array type
      emit("update:tags", [123]);
    });

    it("should handle union types in ModelRef", () => {
      type Model = {
        value: ModelRef<string | number>;
      };

      type Emits = ModelToEmits<Model>;

      const emit: Emits = {} as any;

      emit("update:value", "hello");
      emit("update:value", 42);

      // @ts-expect-error wrong type
      emit("update:value", true);
    });

    it("should return a function for empty model", () => {
      type Model = {};

      type Emits = ModelToEmits<Model>;

      const emit: Emits = {} as any;

      // Empty model should accept any call
      assertType<any>(emit());
    });

    it("should handle single ModelRef property", () => {
      type Model = {
        value: ModelRef<string>;
      };

      type Emits = ModelToEmits<Model>;

      const emit: Emits = {} as any;

      emit("update:value", "hello");

      // @ts-expect-error wrong event
      emit("update:other", "hello");
    });

    it("should handle nullable ModelRef", () => {
      type Model = {
        value: ModelRef<string | null>;
      };

      type Emits = ModelToEmits<Model>;

      const emit: Emits = {} as any;

      emit("update:value", "hello");
      emit("update:value", null);

      // @ts-expect-error wrong type
      emit("update:value", 123);
      // @ts-expect-error undefined is not in the union
      emit("update:value", undefined);
    });

    it("should properly type event names as template literals", () => {
      type Model = {
        firstName: ModelRef<string>;
        lastName: ModelRef<string>;
      };

      type Emits = ModelToEmits<Model>;

      const emit: Emits = {} as any;

      emit("update:firstName", "John");
      emit("update:lastName", "Doe");

      // @ts-expect-error wrong casing
      emit("update:firstname", "John");
      // @ts-expect-error wrong event name
      emit("update:first-name", "John");
    });

    it("should respect custom name in ModelRef for emits", () => {
      type Model = {
        internalValue: ModelRef<string, "value">;
        internalCount: ModelRef<number, "count">;
      };

      type Emits = ModelToEmits<Model>;

      const emit: Emits = {} as any;

      emit("update:value", "hello");
      emit("update:count", 42);

      // @ts-expect-error should use custom name, not property name
      emit("update:internalValue", "hello");
      // @ts-expect-error should use custom name, not property name
      emit("update:internalCount", 42);
    });

    it("should handle mix of custom and default names in emits", () => {
      type Model = {
        internalTitle: ModelRef<string, "title">;
        count: ModelRef<number>;
        internalActive: ModelRef<boolean, "isActive">;
      };

      type Emits = ModelToEmits<Model>;

      const emit: Emits = {} as any;

      emit("update:title", "hello");
      emit("update:count", 42);
      emit("update:isActive", true);

      // @ts-expect-error should use custom name
      emit("update:internalTitle", "hello");
      // @ts-expect-error should use custom name
      emit("update:internalActive", true);
    });

    it("should handle custom names with optional ModelRef in emits", () => {
      // Note: ModelRef<T, Name> | undefined currently doesn't preserve the name remapping
      // This is a TypeScript limitation with conditional types in unions
      type Model = {
        internalValue: ModelRef<string, "value"> | undefined;
      };

      type Emits = ModelToEmits<Model>;

      const emit: Emits = {} as any;

      // The union with undefined prevents name extraction
      emit("update:internalValue", "hello");
      emit("update:internalValue", undefined);
    });

    it("should handle custom names with complex types in emits", () => {
      type Model = {
        internalUser: ModelRef<{ name: string; age: number }, "user">;
        internalTags: ModelRef<string[], "tags">;
      };

      type Emits = ModelToEmits<Model>;

      const emit: Emits = {} as any;

      emit("update:user", { name: "John", age: 30 });
      emit("update:tags", ["vue", "typescript"]);

      // @ts-expect-error should use custom name
      emit("update:internalUser", { name: "John", age: 30 });
      // @ts-expect-error should use custom name
      emit("update:internalTags", ["vue", "typescript"]);
    });

    it("should handle multiple properties with same custom name", () => {
      // This is an edge case - multiple properties mapping to the same name
      // TypeScript will create an intersection of the emit types
      type Model = {
        value1: ModelRef<string, "value">;
        value2: ModelRef<number, "value">;
      };

      type Emits = ModelToEmits<Model>;

      const emit: Emits = {} as any;

      // Both types should be accepted due to intersection
      emit("update:value", "hello");
      emit("update:value", 42);
    });
  });

  describe("ModelToProps and ModelToEmits integration", () => {
    it("should work together for a complete v-model setup", () => {
      type Model = {
        title: ModelRef<string>;
        count: ModelRef<number>;
        isActive: ModelRef<boolean>;
      };

      type Props = ModelToProps<Model>;
      type Emits = ModelToEmits<Model>;

      // Props should have the correct types
      assertType<Props>({ title: "hello", count: 42, isActive: true });

      // Emits should have the correct event signatures
      const emit: Emits = {} as any;
      emit("update:title", "world");
      emit("update:count", 100);
      emit("update:isActive", false);
    });

    it("should handle mixed optional and required models", () => {
      type Model = {
        required: ModelRef<string>;
        optional: ModelRef<number> | undefined;
      };

      type Props = ModelToProps<Model>;
      type Emits = ModelToEmits<Model>;

      assertType<Props>({ required: "hello", optional: 42 });
      assertType<Props>({ required: "hello", optional: undefined });

      const emit: Emits = {} as any;
      emit("update:required", "world");
      emit("update:optional", 100);
      emit("update:optional", undefined);
    });

    it("should work together with custom names", () => {
      type Model = {
        internalTitle: ModelRef<string, "title">;
        internalCount: ModelRef<number, "count">;
        isActive: ModelRef<boolean>;
      };

      type Props = ModelToProps<Model>;
      type Emits = ModelToEmits<Model>;

      // Props should use custom names
      assertType<Props>({ title: "hello", count: 42, isActive: true });

      // Emits should use custom names
      const emit: Emits = {} as any;
      emit("update:title", "world");
      emit("update:count", 100);
      emit("update:isActive", false);

      // @ts-expect-error internal names should not exist in emits
      emit("update:internalTitle", "world");
      // @ts-expect-error internal names should not exist in emits
      emit("update:internalCount", 100);
    });

    it("should handle real-world defineModel scenario", () => {
      // Simulating a component that uses defineModel with custom names
      type Model = {
        modelValue: ModelRef<string>; // default v-model
        count: ModelRef<number, "count">; // v-model:count
        internalState: ModelRef<boolean, "active">; // v-model:active
      };

      type Props = ModelToProps<Model>;
      type Emits = ModelToEmits<Model>;

      // Props
      assertType<Props>({
        modelValue: "hello",
        count: 42,
        active: true,
      });

      // Emits
      const emit: Emits = {} as any;
      emit("update:modelValue", "world");
      emit("update:count", 100);
      emit("update:active", false);
    });

    it("should filter non-ModelRef properties in integration", () => {
      type Model = {
        value: ModelRef<string, "modelValue">;
        regularProp: string;
        count: ModelRef<number>;
        anotherProp: boolean;
      };

      type Props = ModelToProps<Model>;
      type Emits = ModelToEmits<Model>;

      // Only ModelRef properties should be in Props
      type ExpectedProps = {
        modelValue: string;
        count: number;
      };

      assertType<Props>({} as ExpectedProps);
      assertType<ExpectedProps>({} as Props);

      // Only ModelRef properties should be in Emits
      const emit: Emits = {} as any;
      emit("update:modelValue", "hello");
      emit("update:count", 42);

      // @ts-expect-error non-ModelRef properties should not have emits
      emit("update:regularProp", "hello");
      // @ts-expect-error non-ModelRef properties should not have emits
      emit("update:anotherProp", true);
    });
  });
});
