import { describe, it, assertType, expect } from "vitest";
import {
  ReturnMacros,
  RegularMacros,
  MacroReturnType,
  MacroReturnObject,
  MacroReturn,
  createMacroReturn,
  ExtractMacroReturn,
  ExtractMacro,
  ExtractMacroProps,
  ExtractEmits,
  ExtractSlots,
  ExtractOptions,
  ExtractModel,
  ExtractExpose,
  NormaliseMacroReturn,
} from "./setup";
import { PropType } from "vue";

describe("Setup helpers", () => {
  // String
  const stringObject = {
    foo: String,
  };
  type StringObjectType = typeof stringObject;
  const stringValue = {
    foo: "string",
  };
  type StringValueType = typeof stringValue;
  type StringType = {
    foo: string;
  };
  // /String

  // Number
  const numberObject = {
    foo: Number,
  };
  type NumberObjectType = typeof numberObject;
  const numberValue = {
    foo: 42,
  };
  type NumberValueType = typeof numberValue;
  type NumberType = { foo: number };
  // /Number

  // Complex
  const complexObject = {
    test: {
      type: Object as () => { a: number; b: string },
      required: true,
    },
    bar: String,
  };
  type ComplexObjectType = typeof complexObject;
  const complexValue = {
    test: {
      a: 1,
      b: "string",
    },
    bar: "optional",
  };
  type ComplexValueType = typeof complexValue;
  type ComplexType = {
    test: {
      a: number;
      b: string;
    };
    bar?: string;
  };
  // /Complex

  // Generic
  const genericValue = {
    foo: "generic",
  };
  type GenericValueType = typeof genericValue;
  const createGenericObject = <T>() => ({
    foo: {
      type: String as unknown as PropType<() => T>,
      required: true,
    },
  });
  type GenericObjectType<T extends string> = ReturnType<
    typeof createGenericObject<T>
  >;
  type GenericType<T extends string> = {
    foo: T;
  };
  // /Generic

  describe("ReturnMacros", () => {
    it("includes all valid macro names", () => {
      const props: ReturnMacros = "props";
      const emits: ReturnMacros = "emits";
      const slots: ReturnMacros = "slots";
      const options: ReturnMacros = "options";
      const model: ReturnMacros = "model";
      const expose: ReturnMacros = "expose";

      void props;
      void emits;
      void slots;
      void options;
      void model;
      void expose;
    });

    it("rejects invalid macro names", () => {
      // @ts-expect-error invalid macro name
      const invalid: ReturnMacros = "invalid";
      void invalid;
    });
  });

  describe("RegularMacros", () => {
    it("excludes model from regular macros", () => {
      const props: RegularMacros = "props";
      const emits: RegularMacros = "emits";
      const slots: RegularMacros = "slots";
      const options: RegularMacros = "options";
      const expose: RegularMacros = "expose";

      void props;
      void emits;
      void slots;
      void options;
      void expose;
    });

    it("rejects model as regular macro", () => {
      // @ts-expect-error model is not a regular macro
      const model: RegularMacros = "model";
      void model;
    });
  });

  describe("MacroReturnType", () => {
    it("creates type-based macro return structure", () => {
      type StringMacro = MacroReturnType<StringValueType, StringType>;

      assertType<StringMacro["type"]>({} as StringType);
      assertType<StringMacro["value"]>(stringValue);

      assertType<StringMacro["value"]>({} as StringMacro["type"]);
      assertType<StringMacro["value"]>({ foo: "" });
      assertType<StringMacro["type"]>({ foo: "" });

      // @ts-expect-error wrong type
      assertType<StringMacro["value"]>({ foo: 0 });
    });

    it("preserves value and type generics", () => {
      type GenericMacro<T extends string> = MacroReturnType<
        GenericValueType,
        GenericType<T>
      >;

      assertType<GenericMacro<"test">["value"]>(genericValue);
      assertType<GenericMacro<"test">["type"]>({} as GenericType<"test">);

      // @ts-expect-error should keep string literal
      assertType<GenericMacro<"test">["type"]>({ foo: "" });

      // @ts-expect-error should keep string literal
      assertType<GenericMacro<"test">["type"]>({} as GenericType<"foo">);
    });

    it("works with complex value types", () => {
      type ComplexMacro = MacroReturnType<ComplexValueType, ComplexType>;

      assertType<ComplexMacro["value"]>(complexValue);

      assertType<ComplexMacro["value"]>({} as ComplexValueType);
      assertType<ComplexMacro["type"]>({} as ComplexType);

      // @ts-expect-error wrong type
      assertType<ComplexMacro["value"]>({ foo: 0 });

      // @ts-expect-error wrong type
      assertType<ComplexMacro["value"]>({ foo: 0 });
    });

    it("rejects wrong structure with object instead of type", () => {
      type StringMacro = MacroReturnType<StringValueType, StringType>;
      const invalid: StringMacro = {
        value: { foo: "test" },
        // @ts-expect-error wrong property name
        object: "StringType",
      };
      void invalid;
    });
  });

  describe("MacroReturnObject", () => {
    it("creates object-based macro return structure", () => {
      type StringMacro = MacroReturnObject<StringObjectType, StringType>;

      assertType<StringMacro["value"]>({} as StringObjectType);
      assertType<StringMacro["object"]>({} as StringType);

      // @ts-expect-error wrong type
      assertType<StringMacro["value"]>({ foo: "test" });
    });

    it("preserves value and object generics", () => {
      type GenericMacro = MacroReturnObject<
        GenericValueType,
        GenericObjectType<"test">
      >;

      assertType<GenericMacro["value"]>(genericValue);
      assertType<GenericMacro["object"]>(createGenericObject<"test">());

      // @ts-expect-error wrong type
      assertType<GenericMacro["object"]>(createGenericObject<"foo">());
    });

    it("works with complex object types", () => {
      type ComplexMacro = MacroReturnObject<
        ComplexObjectType,
        ComplexObjectType
      >;
      assertType<ComplexMacro["value"]>(complexObject);
      assertType<ComplexMacro["object"]>({} as ComplexObjectType);

      // @ts-expect-error wrong type
      assertType<ComplexMacro["object"]>({} as ComplexType);
    });

    it("rejects wrong structure with type instead of object", () => {
      type StringMacro = MacroReturnObject<StringValueType, StringObjectType>;
      const invalid: StringMacro = {
        value: stringValue,
        // @ts-expect-error wrong property name
        type: "StringObject",
      };
      void invalid;
    });
  });

  describe("MacroReturn", () => {
    it("accepts MacroReturnType structure", () => {
      type TestMacro = MacroReturn<StringValueType, StringType>;
      const macro: TestMacro = {
        value: stringValue,
        type: {} as StringType,
      };

      assertType<TestMacro["value"]>(stringValue);
      void macro;
    });

    it("accepts MacroReturnObject structure", () => {
      type TestMacro = MacroReturn<StringValueType, StringObjectType>;
      const macro: TestMacro = {
        value: stringValue,
        object: stringObject,
      };

      assertType<TestMacro["value"]>(stringValue);
      void macro;
    });

    it("is a union of both return types", () => {
      type TestMacro = MacroReturn<NumberValueType, NumberObjectType>;

      const withType: TestMacro = {
        value: numberValue,
        type: numberObject,
      };

      const withObject: TestMacro = {
        value: numberValue,
        object: numberObject,
      };

      void withType;
      void withObject;
    });

    it("rejects invalid structures", () => {
      type TestMacro = MacroReturn<StringValueType, StringObjectType>;

      // @ts-expect-error missing value
      const noValue: TestMacro = {
        type: stringObject,
      };

      // @ts-expect-error missing type or object
      const noTypeOrObject: TestMacro = {
        value: stringValue,
      };

      void noValue;
      void noTypeOrObject;
    });
  });

  describe("ExtractMacroReturn", () => {
    it("extracts the wrapped type from createMacroReturn", () => {
      const result = createMacroReturn({
        props: { value: { id: 1 }, type: "Props" },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      type Expected = { props: { value: { id: number }; type: string } };

      assertType<Extracted>({} as Expected);
    });

    it("returns never for non-macro types", () => {
      type NotMacro = { foo: string };
      type Extracted = ExtractMacroReturn<NotMacro>;

      assertType<Extracted>({} as never);
    });
  });

  describe("ExtractMacro", () => {
    it("extracts a specific macro when present", () => {
      type Macros = { props: { value: string; type: string } };
      type Props = ExtractMacro<Macros, "props">;

      assertType<Props["value"]>("test");
      assertType<Props["type"]>("string");

      // @ts-expect-error nonExistent property
      assertType<Props["nonExistent"]>("");

      // @ts-expect-error incorrect type
      assertType<Props["value"]>(1);
    });

    it("returns never for missing macro without fallback", () => {
      type Macros = { props: { value: string; type: string } };
      type Emits = ExtractMacro<Macros, "emits">;

      assertType<Emits>({} as never);

      // @ts-expect-error nonExistent property
      assertType<Emits["nonExistent"]>("");
    });

    it("returns fallback for missing macro with fallback", () => {
      type Macros = { props: { value: string; type: string } };
      type Emits = ExtractMacro<Macros, "emits", () => void>;

      assertType<Emits>(() => {});

      // @ts-expect-error nonExistent property
      assertType<Emits["nonExistent"]>("");
    });

    it("extracts model structure", () => {
      type Macros = {
        model: {
          value: { value: string; type: string };
          count: { value: number; type: number };
        };
      };
      type Model = ExtractMacro<Macros, "model">;

      assertType<Model["value"]["value"]>("test");
      assertType<Model["count"]["value"]>(42);

      // @ts-expect-error nonExistent property
      assertType<Model["nonExistent"]>("");

      // @ts-expect-error incorrect type
      assertType<Model["value"]["value"]>(1);
    });
  });

  describe("ExtractProps", () => {
    it("extracts props macro without defaults field", () => {
      type Macros = { props: { value: { id: number }; type: string } };
      type Props = ExtractMacroProps<Macros>;

      assertType<Props["props"]["value"]>({ id: 1 });
      assertType<Props["props"]["type"]>("string");
      // @ts-expect-error missing defaults field
      assertType<Props["props"]["defaults"]>({} as any);

      // @ts-expect-error nonExistent property
      assertType<Props["props"]["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("returns empty object with defaults when props not present", () => {
      type Macros = { emits: { value: () => void; type: string } };
      type Props = ExtractMacroProps<Macros>;

      assertType<Props["props"]>({});

      assertType<Props["defaults"]>({});

      // @ts-expect-error nonExistent property
      assertType<Props["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Props["value"]>({ id: "wrong type" });
    });

    it("extracts withDefaults into defaults field", () => {
      type Macros = {
        props: { value: { id: number }; type: string };
        withDefaults: { id: 1 };
      };
      type Props = ExtractMacroProps<Macros>;
      assertType<Props["props"]>({ value: { id: 1 }, type: "string" });
      assertType<Props["defaults"]>({ id: 1 });

      // @ts-expect-error nonExistent property
      assertType<Props["props"]["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });
  });

  describe("ExtractEmits", () => {
    it("extracts emits macro", () => {
      type Macros = { emits: { value: () => void; object: string } };
      type Emits = ExtractEmits<Macros>;

      assertType<Emits["value"]>(() => {});
      assertType<Emits["object"]>("EmitsObject");

      // @ts-expect-error nonExistent property
      assertType<Emits["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Emits["value"]>(1);
    });

    it("returns default emit function when emits not present", () => {
      type Macros = { props: { value: {}; type: string } };
      type Emits = ExtractEmits<Macros>;

      assertType<Emits>(() => {});

      // @ts-expect-error nonExistent property
      assertType<Emits["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Emits["value"]>(1);
    });

    it("handles complex emit types", () => {
      type EmitFn = (event: "update", value: string) => void;
      type Macros = { emits: { value: EmitFn; type: string } };
      type Emits = ExtractEmits<Macros>;
      assertType<Emits["value"]>((event: "update", value: string) => {});


      assertType<Emits["type"]>("EmitsType");

      // @ts-expect-error nonExistent property
      assertType<Emits["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Emits["value"]>(1);
    });
  });

  describe("ExtractSlots", () => {
    it("extracts slots macro", () => {
      type Macros = {
        slots: { value: { default: () => void }; type: string };
      };
      type Slots = ExtractSlots<Macros>;
      assertType<Slots["value"]["default"]>(() => {});
      assertType<Slots["type"]>("SlotsType");

      // @ts-expect-error nonExistent property
      assertType<Slots["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Slots["value"]>(1);
    });

    it("returns empty object when slots not present", () => {
      type Macros = { props: { value: {}; type: string } };
      type Slots = ExtractSlots<Macros>;

      assertType<Slots>({});

      // @ts-expect-error nonExistent property
      assertType<Slots["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Slots["value"]>(1);
    });

    it("handles multiple slot types", () => {
      type Macros = {
        slots: {
          value: {
            default: () => void;
            header: () => void;
            footer: () => void;
          };
          type: string;
        };
      };
      type Slots = ExtractSlots<Macros>;

      assertType<Slots["value"]["default"]>(() => {});
      assertType<Slots["value"]["header"]>(() => {});
      assertType<Slots["value"]["footer"]>(() => {});

      // @ts-expect-error nonexistent slot property
      assertType<Slots["value"]["foo"]>(() => {});

      // @ts-expect-error nonExistent property
      assertType<Slots["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Slots["value"]>(1);
    });
  });

  describe("ExtractOptions", () => {
    it("extracts options macro", () => {
      type Macros = {
        options: { value: { name: string; version: number }; type: string };
      };
      type Options = ExtractOptions<Macros>;

      assertType<Options["value"]["name"]>("Test");
      assertType<Options["value"]["version"]>(1);
      assertType<Options["type"]>("OptionsType");

      // @ts-expect-error nonExistent property
      assertType<Options["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Options["value"]>(1);
    });

    it("returns empty object when options not present", () => {
      type Macros = { props: { value: {}; type: string } };
      type Options = ExtractOptions<Macros>;

      assertType<Options>({});

      // @ts-expect-error nonExistent property
      assertType<Options["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Options["value"]>(1);
    });
  });

  describe("ExtractModel", () => {
    it("extracts model macro with nested keys", () => {
      type Macros = {
        model: {
          modelValue: { value: string; type: string };
          count: { value: number; type: number };
        };
      };
      type Model = ExtractModel<Macros>;

      assertType<Model["modelValue"]["value"]>("test");
      assertType<Model["count"]["value"]>(42);

      // @ts-expect-error nonExistent property
      assertType<Model["count"]["foo"]>("");

      // @ts-expect-error nonExistent property
      assertType<Model["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Model["value"]>(1);
    });

    it("returns empty object when model not present", () => {
      type Macros = { props: { value: {}; type: string } };
      type Model = ExtractModel<Macros>;

      assertType<Model>({});

      // @ts-expect-error nonExistent property
      assertType<Model["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Model["value"]>(1);
    });

    it("handles single model value", () => {
      type Macros = {
        model: {
          value: { value: string; type: string };
        };
      };
      type Model = ExtractModel<Macros>;

      assertType<Model["value"]["value"]>("test");

      // @ts-expect-error nonexistent nested property
      assertType<Model["value"]["foo"]>("");

      // @ts-expect-error nonExistent property
      assertType<Model["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Model["value"]>(1);
    });
  });

  describe("ExtractExpose", () => {
    it("extracts expose macro", () => {
      type Macros = {
        expose: {
          value: { focus: () => void; blur: () => void };
          type: string;
        };
      };
      type Expose = ExtractExpose<Macros>;
      assertType<Expose["value"]["focus"]>(() => {});
      assertType<Expose["value"]["blur"]>(() => {});
      assertType<Expose["type"]>("ExposeType");

      // @ts-expect-error nonexistent nested property
      assertType<Expose["value"]["foo"]>(() => {});

      // @ts-expect-error nonExistent property
      assertType<Expose["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Expose["value"]>(1);
    });

    it("returns empty object when expose not present", () => {
      type Macros = { props: { value: {}; type: string } };
      type Expose = ExtractExpose<Macros>;

      assertType<Expose>({});

      // @ts-expect-error nonExistent property
      assertType<Expose["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Expose["value"]>(1);
    });

    it("handles complex exposed API", () => {
      type Macros = {
        expose: {
          value: {
            focus: () => void;
            getValue: () => string;
            setValue: (val: string) => void;
          };
          type: string;
        };
      };
      type Expose = ExtractExpose<Macros>;
      const expose: Expose = {
        value: {
          focus: () => {},
          getValue: () => "value",
          setValue: (val: string) => {},
        },
        type: "ExposeType",
      };

      assertType<Expose["value"]["focus"]>(() => {});
      assertType<Expose["value"]["getValue"]>(() => "value");
      assertType<Expose["value"]["setValue"]>((val: string) => {});
      // @ts-expect-error nonexistent nested property
      assertType<Expose["value"]["foo"]>(() => {});

      // @ts-expect-error nonExistent property
      assertType<Expose["nonExistent"]>("");
      // @ts-expect-error incorrect type
      assertType<Expose["value"]>(1);
    });
  });

  describe("NormaliseMacroReturn", () => {
    it("normalizes macro return with all macros present", () => {
      const result = createMacroReturn({
        props: { value: { id: 1 }, type: "Props" },
        emits: { value: () => {}, type: "Emits" },
        slots: { value: {}, type: "Slots" },
        options: { value: { name: "Test" }, type: "Options" },
        model: { value: { value: "test", type: "string" } },
        expose: { value: { focus: () => {} }, type: "Expose" },
      });

      type Normalized = NormaliseMacroReturn<typeof result>;

      assertType<Normalized["props"]["props"]["value"]>({} as { id: number });
      assertType<Normalized["props"]["props"]["type"]>("Props");
      assertType<Normalized["props"]["defaults"]>({} as {});
      assertType<Normalized["emits"]["value"]>({} as () => void);
      assertType<Normalized["slots"]["value"]>({} as {});
      assertType<Normalized["options"]["value"]>({} as { name: string });
      assertType<Normalized["model"]["value"]["value"]>({} as string);
      assertType<Normalized["expose"]["value"]["focus"]>({} as () => void);

      // @ts-expect-error incorrect type
      assertType<Normalized["props"]["props"]["value"]>({ id: "wrong type" });
    });

    it("provides defaults for missing macros", () => {
      const result = createMacroReturn({
        props: { value: { id: 1 }, type: "Props" },
      });

      type Normalized = NormaliseMacroReturn<typeof result>;

      assertType<Normalized["props"]["props"]["value"]>({} as { id: number });
      assertType<Normalized["props"]["props"]["type"]>("Props");
      assertType<Normalized["emits"]>({} as () => void);
      assertType<Normalized["slots"]>({} as {});
      assertType<Normalized["options"]>({} as {});
      assertType<Normalized["model"]>({} as {});
      assertType<Normalized["expose"]>({} as {});

      // @ts-expect-error incorrect type
      assertType<Normalized["props"]["props"]["value"]>({ id: "wrong type" });
    });

    it("normalizes empty macro return", () => {
      const result = createMacroReturn({});
      type Normalized = NormaliseMacroReturn<typeof result>;

      assertType<Normalized["props"]["props"]>({});
      assertType<Normalized["props"]["defaults"]>({});
      assertType<Normalized["emits"]>({} as () => void);
      assertType<Normalized["slots"]>({} as {});
      assertType<Normalized["options"]>({} as {});
      assertType<Normalized["model"]>({} as {});
      assertType<Normalized["expose"]>({} as {});

      // @ts-expect-error incorrect type
      assertType<Normalized["props"]["props"]["value"]>({ id: "wrong type" });
    });

    it("normalizes partial macro return", () => {
      const result = createMacroReturn({
        props: { value: { count: 0 }, type: "number" },
        model: {
          modelValue: { value: "test", type: "string" },
        },
      });

      type Normalized = NormaliseMacroReturn<typeof result>;

      assertType<Normalized["props"]["props"]["value"]>(
        {} as { count: number }
      );
      assertType<Normalized["props"]["props"]["type"]>("number");
      assertType<Normalized["model"]["modelValue"]["value"]>({} as string);
      assertType<Normalized["emits"]>({} as () => void);
      assertType<Normalized["slots"]>({} as {});
      assertType<Normalized["options"]>({} as {});
      assertType<Normalized["expose"]>({} as {});

      // @ts-expect-error incorrect type
      assertType<Normalized["props"]["defaults"]["test"]>({
        count: "wrong type",
      });
    });

    it("preserves complex prop types", () => {
      type ComplexProps = {
        user: { id: number; name: string };
        settings: { theme: "light" | "dark" };
      };

      const result = createMacroReturn({
        props: {
          value: {
            user: { id: 1, name: "John" },
            settings: { theme: "dark" as "light" | "dark" },
          },
          type: "ComplexProps",
        },
      });

      type Normalized = NormaliseMacroReturn<typeof result>;
      assertType<Normalized["props"]["props"]["value"]>({} as ComplexProps);

      // @ts-expect-error incorrect type
      assertType<Normalized["props"]["props"]["test"]>({
        settings: { theme: "dark" },
      });
    });

    it("preserves multiple model keys", () => {
      const result = createMacroReturn({
        model: {
          firstName: { value: "John", type: "string" },
          lastName: { value: "Doe", type: "string" },
          age: { value: 30, type: "number" },
        },
      });

      type Normalized = NormaliseMacroReturn<typeof result>;
      type Model = Normalized["model"];

      assertType<Model["firstName"]["value"]>({} as string);
      assertType<Model["lastName"]["value"]>({} as string);
      assertType<Model["age"]["value"]>({} as number);

      // @ts-expect-error incorrect type
      assertType<Model["age"]["value"]>({} as string);
    });

    it("handles non-macro type", () => {
      type NonMacro = { foo: string };
      type Normalized = NormaliseMacroReturn<NonMacro>;

      // Non-macro types result in never for all fields since ExtractMacroReturn returns never
      assertType<Normalized["props"]>({} as never);
      assertType<Normalized["emits"]>({} as never);
      assertType<Normalized["slots"]>({} as never);
      assertType<Normalized["options"]>({} as never);
      assertType<Normalized["model"]>({} as never);
      assertType<Normalized["expose"]>({} as never);

      // @ts-expect-error incorrect type
      assertType<Normalized["props"]["props"]>({} as {});
    });
  });

  describe("createMacroReturn", () => {
    it("creates empty macro return", () => {
      const result = createMacroReturn({});
      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted>({} as {});

      // @ts-expect-error nonExistent property
      assertType<Extracted["nonExistent"]>("");
    });

    it("creates macro return with props", () => {
      const result = createMacroReturn({
        props: { value: { id: 1 }, type: "PropsType" },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["props"]["value"]>({} as { id: number });
      assertType<Extracted["props"]["type"]>({} as "PropsType");

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("creates macro return with emits", () => {
      const result = createMacroReturn({
        emits: { value: () => {}, object: "EmitsObject" },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["emits"]["value"]>({} as () => void);
      assertType<Extracted["emits"]["object"]>({} as "EmitsObject");

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("creates macro return with slots", () => {
      const result = createMacroReturn({
        slots: { value: {}, type: "SlotsType" },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["slots"]["value"]>({} as {});
      assertType<Extracted["slots"]["type"]>({} as "SlotsType");

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("creates macro return with options", () => {
      const result = createMacroReturn({
        options: { value: { name: "Test" }, object: { name: "Test" } },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["options"]["value"]>({} as { name: string });
      assertType<Extracted["options"]["object"]>({} as { name: string });
      // @ts-expect-error incorrect type
      assertType<Extracted["options"]["value"]>({ name: 123 });
    });

    it("creates macro return with expose", () => {
      const result = createMacroReturn({
        expose: { value: { method: () => {} }, type: "ExposeType" },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["expose"]["value"]>({} as { method: () => void });
      assertType<Extracted["expose"]["type"]>({} as "ExposeType");

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("creates macro return with model object", () => {
      const result = createMacroReturn({
        model: {
          value: { value: "test", type: "string" },
          title: { value: "Title", object: { type: "string" } },
        },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      type ModelType = Extracted["model"];
      assertType<ModelType["value"]["value"]>({} as string);
      assertType<ModelType["value"]["type"]>({} as "string");
      assertType<ModelType["title"]["value"]>({} as string);
      assertType<ModelType["title"]["object"]>({} as { type: string });

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("creates macro return with multiple regular macros", () => {
      const result = createMacroReturn({
        props: { value: { id: 1 }, type: "Props" },
        emits: { value: () => {}, type: "Emits" },
        slots: { value: {}, type: "Slots" },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["props"]["value"]>({} as { id: number });
      assertType<Extracted["props"]["type"]>({} as "Props");
      assertType<Extracted["emits"]["value"]>({} as () => void);
      assertType<Extracted["emits"]["type"]>({} as "Emits");
      assertType<Extracted["slots"]["value"]>({} as {});
      assertType<Extracted["slots"]["type"]>({} as "Slots");

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("creates macro return with all macros", () => {
      const result = createMacroReturn({
        props: { value: { id: 1 }, type: "Props" },
        emits: { value: () => {}, type: "Emits" },
        slots: { value: {}, type: "Slots" },
        options: { value: { name: "Test" }, type: "Options" },
        expose: { value: { method: () => {} }, type: "Expose" },
        model: {
          value: { value: "test", type: "string" },
        },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["props"]["value"]>({} as { id: number });
      assertType<Extracted["props"]["type"]>({} as "Props");
      assertType<Extracted["emits"]["value"]>({} as () => void);
      assertType<Extracted["emits"]["type"]>({} as "Emits");
      assertType<Extracted["slots"]["value"]>({} as {});
      assertType<Extracted["slots"]["type"]>({} as "Slots");
      assertType<Extracted["options"]["value"]>({} as { name: string });
      assertType<Extracted["options"]["type"]>({} as "Options");
      assertType<Extracted["expose"]["value"]>({} as { method: () => void });
      assertType<Extracted["expose"]["type"]>({} as "Expose");
      assertType<Extracted["model"]["value"]["value"]>({} as string);
      assertType<Extracted["model"]["value"]["type"]>({} as "string");

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("preserves partial structure", () => {
      const result = createMacroReturn({
        props: { value: { id: 1 }, type: "Props" },
        // emits omitted
        slots: { value: {}, type: "Slots" },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      type HasProps = "props" extends keyof Extracted ? true : false;
      type HasEmits = "emits" extends keyof Extracted ? true : false;
      type HasSlots = "slots" extends keyof Extracted ? true : false;

      assertType<HasProps>({} as true);
      assertType<HasEmits>({} as false);
      assertType<HasSlots>({} as true);

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("allows nested model with multiple keys", () => {
      const result = createMacroReturn({
        model: {
          firstName: { value: "John", type: "string" },
          lastName: { value: "Doe", type: "string" },
          age: { value: 30, type: "number" },
        },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      type ModelType = Extracted["model"];
      assertType<ModelType["firstName"]["value"]>({} as string);
      assertType<ModelType["firstName"]["type"]>({} as "string");
      assertType<ModelType["lastName"]["value"]>({} as string);
      assertType<ModelType["lastName"]["type"]>({} as "string");
      assertType<ModelType["age"]["value"]>({} as number);
      assertType<ModelType["age"]["type"]>({} as "number");

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("preserves exact return type", () => {
      const input = {
        props: { value: { id: 1 }, type: "Props" as const },
        emits: { value: () => {}, object: "Emits" as const },
      };
      const result = createMacroReturn(input);

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted>({} as typeof input);

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("handles mixed type and object returns", () => {
      const result = createMacroReturn({
        props: { value: { id: 1 }, type: "PropsType" },
        emits: { value: () => {}, object: "EmitsObject" },
        slots: { value: {}, type: "SlotsType" },
        options: { value: { name: "Test" }, object: { name: "Test" } },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["props"]["type"]>({} as "PropsType");
      assertType<Extracted["emits"]["object"]>({} as "EmitsObject");

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("rejects non-regular macros at model level", () => {
      const result = createMacroReturn({
        // @ts-expect-error model cannot be a direct MacroReturn
        model: { value: "test", type: "ModelType" },
      });
      // @ts-expect-error incorrect type
      assertType<ExtractMacroReturn<typeof result>["model"]["value"]>(
        "wrong type"
      );
    });

    it("handles empty model object", () => {
      const result = createMacroReturn({
        model: {},
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      type ModelType = Extracted["model"];
      assertType<ModelType>({} as {});

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("handles complex nested value types", () => {
      type ComplexProps = {
        user: { id: number; name: string };
        settings: { theme: "light" | "dark" };
      };

      const result = createMacroReturn({
        props: {
          value: {
            user: { id: 1, name: "John" },
            settings: { theme: "dark" as "light" | "dark" },
          },
          type: "ComplexPropsType",
        },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["props"]["value"]>({} as ComplexProps);

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });

    it("handles function value types", () => {
      type EmitFn = (event: string, ...args: any[]) => void;

      const result = createMacroReturn({
        emits: {
          value: ((event: string, ...args: any[]) => {}) as EmitFn,
          type: "EmitFunction",
        },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["emits"]["value"]>({} as EmitFn);

      // @ts-expect-error incorrect type
      assertType<Extracted["props"]["value"]>({ id: "wrong type" });
    });
  });

  describe("ExtractMacro", () => {
    it("extracts specific macro from metadata", () => {
      type Macros = {
        props: MacroReturn<{ id: number }, "Props">;
        emits: MacroReturn<() => void, "Emits">;
      };

      type Props = ExtractMacro<Macros, "props">;
      type Emits = ExtractMacro<Macros, "emits">;

      assertType<Props>({} as MacroReturn<{ id: number }, "Props">);
      assertType<Emits>({} as MacroReturn<() => void, "Emits">);

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("returns never for missing macro without fallback", () => {
      type Macros = {
        props: MacroReturn<{ id: number }, "Props">;
      };

      type Emits = ExtractMacro<Macros, "emits">;
      assertType<Emits>({} as never);

      // @ts-expect-error incorrect type
      assertType<Emits["props"]["value"]>({ id: "wrong type" });
    });

    it("returns fallback type for missing macro", () => {
      type Macros = {
        props: MacroReturn<{ id: number }, "Props">;
      };

      type Emits = ExtractMacro<Macros, "emits", () => void>;
      assertType<Emits>({} as () => void);

      type Slots = ExtractMacro<Macros, "slots", {}>;
      assertType<Slots>({} as {});

      // @ts-expect-error incorrect type
      assertType<Emits["props"]["value"]>({ id: "wrong type" });
    });

    it("preserves macro structure", () => {
      type Macros = {
        props: MacroReturnType<{ foo: string }, "PropsType">;
        emits: MacroReturnObject<() => void, { onChange: [] }>;
      };

      type Props = ExtractMacro<Macros, "props">;
      type Emits = ExtractMacro<Macros, "emits">;

      assertType<Props>({} as MacroReturnType<{ foo: string }, "PropsType">);
      assertType<Emits>({} as MacroReturnObject<() => void, { onChange: [] }>);

      // @ts-expect-error incorrect type
      assertType<Emits["props"]["value"]>({ id: "wrong type" });
    });

    it("works with all macro types", () => {
      type Macros = {
        props: MacroReturn<any, any>;
        emits: MacroReturn<any, any>;
        slots: MacroReturn<any, any>;
        options: MacroReturn<any, any>;
        model: Record<string, MacroReturn<any, any>>;
        expose: MacroReturn<any, any>;
      };

      type Props = ExtractMacro<Macros, "props">;
      type Emits = ExtractMacro<Macros, "emits">;
      type Slots = ExtractMacro<Macros, "slots">;
      type Options = ExtractMacro<Macros, "options">;
      type Model = ExtractMacro<Macros, "model">;
      type Expose = ExtractMacro<Macros, "expose">;

      // All should extract their respective macro types
      assertType<Props>({} as MacroReturn<any, any>);
      assertType<Emits>({} as MacroReturn<any, any>);
      assertType<Slots>({} as MacroReturn<any, any>);
      assertType<Options>({} as MacroReturn<any, any>);
      assertType<Model>({} as Record<string, MacroReturn<any, any>>);
      assertType<Expose>({} as MacroReturn<any, any>);

      // @ts-expect-error incorrect type
      assertType<Emits["props"]["value"]>({ id: "wrong type" });

      //@ts-expect-error wrong-type
      assertType<Props["foo"]>({});
      //@ts-expect-error wrong-type
      assertType<Emits["foo"]>({});
      //@ts-expect-error wrong-type
      assertType<Slots["foo"]>({});
      //@ts-expect-error wrong-type
      assertType<Options["foo"]>({});
      //@ts-expect-error wrong-type
      assertType<Model["foo"]>({});
      //@ts-expect-error wrong-type
      assertType<Expose["foo"]>({});
    });

    it("handles empty metadata object", () => {
      type Macros = {};

      type Props = ExtractMacro<Macros, "props", {}>;
      assertType<Props>({} as {});

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("handles partial metadata", () => {
      type Macros = {
        props: MacroReturn<{ id: number }, "Props">;
        slots: MacroReturn<{}, "Slots">;
      };

      type Props = ExtractMacro<Macros, "props">;
      type Emits = ExtractMacro<Macros, "emits", () => void>;
      type Slots = ExtractMacro<Macros, "slots">;

      assertType<Props>({} as MacroReturn<{ id: number }, "Props">);
      assertType<Emits>({} as () => void); // fallback
      assertType<Slots>({} as MacroReturn<{}, "Slots">);

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });
  });

  describe("ExtractProps", () => {
    it("extracts props from macro metadata", () => {
      type Macros = {
        props: MacroReturn<{ id: number; name: string }, "Props">;
      };

      type Props = ExtractMacroProps<Macros>;
      assertType<Props>(
        {} as { props: MacroReturn<{ id: number; name: string }, "Props"> } & {
          defaults: {};
        }
      );

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("returns empty object for missing props", () => {
      type Macros = {
        emits: MacroReturn<() => void, "Emits">;
      };

      type Props = ExtractMacroProps<Macros>;
      assertType<Props>({} as { props: {} } & { defaults: {} });

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("works with complex prop types", () => {
      type ComplexProps = {
        user: { id: number; name: string };
        settings: { theme: "light" | "dark" };
        tags: string[];
      };

      type Macros = {
        props: MacroReturnType<ComplexProps, "ComplexPropsType">;
      };

      type Props = ExtractMacroProps<Macros>;
      assertType<Props>(
        {} as { props: MacroReturnType<ComplexProps, "ComplexPropsType"> } & {
          defaults: {};
        }
      );

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("preserves MacroReturnType vs MacroReturnObject", () => {
      type Macros1 = {
        props: MacroReturnType<{ id: number }, "PropsType">;
      };
      type Macros2 = {
        props: MacroReturnObject<{ id: number }, { id: "number" }>;
      };

      type Props1 = ExtractMacroProps<Macros1>;
      type Props2 = ExtractMacroProps<Macros2>;

      assertType<Props1>(
        {} as { props: MacroReturnType<{ id: number }, "PropsType"> } & {
          defaults: {};
        }
      );
      assertType<Props2>(
        {} as { props: MacroReturnObject<{ id: number }, { id: "number" }> } & {
          defaults: {};
        }
      );

      // @ts-expect-error incorrect type
      assertType<Props1["props"]["value"]>({ id: "wrong type" });

      // @ts-expect-error incorrect type
      assertType<Props2["props"]["value"]>({ id: "wrong type" });
    });

    it("works with empty metadata", () => {
      type Macros = {};
      type Props = ExtractMacroProps<Macros>;
      assertType<Props>({} as { props: {} } & { defaults: {} });

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("includes defaults property from withDefaults macro", () => {
      type Macros = {
        props: MacroReturn<{ id: number; name?: string }, "Props">;
        withDefaults: MacroReturn<{ name: string }, "Defaults">;
      };

      type Props = ExtractMacroProps<Macros>;
      assertType<Props>(
        {} as { props: MacroReturn<{ id: number; name?: string }, "Props"> } & {
          defaults: MacroReturn<{ name: string }, "Defaults">;
        }
      );

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("defaults property is empty object when withDefaults missing", () => {
      type Macros = {
        props: MacroReturn<{ id: number }, "Props">;
      };

      type Props = ExtractMacroProps<Macros>;
      assertType<Props>(
        {} as { props: MacroReturn<{ id: number }, "Props"> } & { defaults: {} }
      );

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("extracts defaults property with complex types", () => {
      type PropsType = {
        theme?: "light" | "dark";
        count?: number;
        user?: { id: number; name: string };
      };

      type DefaultsType = {
        theme: "light";
        count: number;
      };

      type Macros = {
        props: MacroReturnType<PropsType, "PropsType">;
        withDefaults: MacroReturnType<DefaultsType, "DefaultsType">;
      };

      type Props = ExtractMacroProps<Macros>;

      assertType<Props>(
        {} as { props: MacroReturnType<PropsType, "PropsType"> } & {
          defaults: MacroReturnType<DefaultsType, "DefaultsType">;
        }
      );

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("works with MacroReturnObject in defaults", () => {
      type Macros = {
        props: MacroReturnObject<
          { id: number; name?: string },
          { id: "number"; name: "string" }
        >;
        withDefaults: MacroReturnObject<
          { name: string },
          { name: '"default"' }
        >;
      };

      type Props = ExtractMacroProps<Macros>;

      assertType<Props>(
        {} as {
          props: MacroReturnObject<
            { id: number; name?: string },
            { id: "number"; name: "string" }
          >;
        } & {
          defaults: MacroReturnObject<{ name: string }, { name: '"default"' }>;
        }
      );
      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });
  });

  describe("Extract withDefaults", () => {
    it("extracts withDefaults from macro metadata", () => {
      type Macros = {
        withDefaults: MacroReturn<{ foo: string; bar: number }, "Defaults">;
      };

      type Defaults = ExtractMacro<Macros, "withDefaults">;
      assertType<Defaults>(
        {} as MacroReturn<{ foo: string; bar: number }, "Defaults">
      );

      // @ts-expect-error incorrect type
      assertType<Defaults["props"]["value"]>({ id: "wrong type" });
    });

    it("returns empty object fallback for missing withDefaults", () => {
      type Macros = {
        props: MacroReturn<{}, "Props">;
      };

      type Defaults = ExtractMacro<Macros, "withDefaults", {}>;
      assertType<Defaults>({} as {});
      // @ts-expect-error incorrect type
      assertType<Defaults["props"]["value"]>({ id: "wrong type" });
    });

    it("works with MacroReturnType for defaults", () => {
      type DefaultsType = {
        theme: "light";
        timeout: number;
        enabled: boolean;
      };

      type Macros = {
        withDefaults: MacroReturnType<DefaultsType, "DefaultsType">;
      };

      type Defaults = ExtractMacro<Macros, "withDefaults">;
      assertType<Defaults>({} as MacroReturnType<DefaultsType, "DefaultsType">);
      // @ts-expect-error incorrect type
      assertType<Defaults["props"]["value"]>({ id: "wrong type" });
    });

    it("works with MacroReturnObject for defaults", () => {
      type DefaultsValue = { foo: string; bar: number };
      type DefaultsObject = { foo: '"hello"'; bar: "42" };

      type Macros = {
        withDefaults: MacroReturnObject<DefaultsValue, DefaultsObject>;
      };

      type Defaults = ExtractMacro<Macros, "withDefaults">;
      assertType<Defaults>(
        {} as MacroReturnObject<DefaultsValue, DefaultsObject>
      );
      // @ts-expect-error incorrect type
      assertType<Defaults["props"]["value"]>({ id: "wrong type" });
    });

    it("works together with props in createMacroReturn", () => {
      const setup = () =>
        createMacroReturn({
          props: {
            value: { id: 0 as number, name: "" as string | undefined },
            type: {} as { id: number; name?: string },
          },
          withDefaults: {
            value: { name: "default" as string },
            type: {} as { name: string },
          },
        });

      type Macros = ExtractMacroReturn<ReturnType<typeof setup>>;

      assertType<Macros>(
        {} as {
          props: {
            value: { id: number; name: string | undefined };
            type: { id: number; name?: string };
          };
          withDefaults: {
            value: { name: string };
            type: { name: string };
          };
        }
      );
      // @ts-expect-error incorrect type
      assertType<Macros["props"]["value"]>({ id: "wrong type" });
    });

    it("ExtractProps includes withDefaults in defaults property", () => {
      const setup = () =>
        createMacroReturn({
          props: {
            value: { id: 0 as number, label: "" as string | undefined },
            type: {} as { id: number; label?: string },
          },
          withDefaults: {
            value: { label: "default" as string },
            type: {} as { label: string },
          },
        });

      type Macros = ExtractMacroReturn<ReturnType<typeof setup>>;
      type Props = ExtractMacroProps<Macros>;

      // Props should have both the props macro and the defaults property
      assertType<Props["defaults"]>(
        {} as {
          value: { label: string };
          type: { label: string };
        }
      );

      // @ts-expect-error incorrect type
      assertType<Macros["props"]["value"]>({ id: "wrong type" });
    });

    it("defaults property accessible from ExtractProps result", () => {
      type Macros = {
        props: {
          value: { foo: number; bar?: string };
          type: { foo: number; bar?: string };
        };
        withDefaults: {
          value: { bar: string };
          type: { bar: string };
        };
      };

      type Props = ExtractMacroProps<Macros>;

      // Access the props part
      assertType<Props["props"]["value"]>({} as { foo: number; bar?: string });

      // Access the defaults part
      assertType<Props["defaults"]>(
        {} as {
          value: { bar: string };
          type: { bar: string };
        }
      );

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("works with nested defaults structure", () => {
      const setup = () =>
        createMacroReturn({
          props: {
            value: {
              config: {} as { theme?: "light" | "dark"; debug?: boolean },
              data: [] as Array<{ id: number }>,
            },
            type: {} as {
              config?: { theme?: "light" | "dark"; debug?: boolean };
              data?: Array<{ id: number }>;
            },
          },
          withDefaults: {
            value: {
              config: { theme: "light" as const, debug: false },
            },
            type: {} as {
              config: { theme: "light"; debug: boolean };
            },
          },
        });

      type Macros = ExtractMacroReturn<ReturnType<typeof setup>>;
      type Props = ExtractMacroProps<Macros>;

      assertType<Props["defaults"]["value"]>(
        {} as {
          config: { theme: "light"; debug: boolean };
        }
      );

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("handles empty withDefaults gracefully", () => {
      const setup = () =>
        createMacroReturn({
          props: {
            value: { id: 0 as number },
            type: {} as { id: number },
          },
          withDefaults: {
            value: {},
            type: {} as {},
          },
        });

      type Macros = ExtractMacroReturn<ReturnType<typeof setup>>;
      type Props = ExtractMacroProps<Macros>;

      assertType<Props["defaults"]>(
        {} as {
          value: {};
          type: {};
        }
      );

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });
  });

  describe("ExtractEmits", () => {
    it("extracts emits from macro metadata", () => {
      type Macros = {
        emits: MacroReturn<(event: string) => void, "Emits">;
      };

      type Emits = ExtractEmits<Macros>;
      assertType<Emits>({} as MacroReturn<(event: string) => void, "Emits">);

      // @ts-expect-error incorrect type
      assertType<Emits["props"]["value"]>({ id: "wrong type" });
    });

    it("returns function fallback for missing emits", () => {
      type Macros = {
        props: MacroReturn<{}, "Props">;
      };

      type Emits = ExtractEmits<Macros>;
      assertType<Emits>({} as () => void);
    });

    it("preserves emit function signature", () => {
      type EmitFn = {
        (event: "change", value: number): void;
        (event: "update", data: { id: number }): void;
      };

      type Macros = {
        emits: MacroReturnType<EmitFn, "EmitsType">;
      };

      type Emits = ExtractEmits<Macros>;
      assertType<Emits>({} as MacroReturnType<EmitFn, "EmitsType">);
      // @ts-expect-error incorrect type
      assertType<Emits["props"]["value"]>({ id: "wrong type" });
    });

    it("works with object-based emit specification", () => {
      type EmitsObject = {
        change: [value: number];
        update: [data: string];
      };

      type Macros = {
        emits: MacroReturnObject<() => void, EmitsObject>;
      };

      type Emits = ExtractEmits<Macros>;
      assertType<Emits>({} as MacroReturnObject<() => void, EmitsObject>);

      // @ts-expect-error incorrect type
      assertType<Emits["props"]["value"]>({ id: "wrong type" });
    });
  });

  describe("ExtractSlots", () => {
    it("extracts slots from macro metadata", () => {
      type Macros = {
        slots: MacroReturn<{}, "Slots">;
      };

      type Slots = ExtractSlots<Macros>;
      assertType<Slots>({} as MacroReturn<{}, "Slots">);

      // @ts-expect-error incorrect type
      assertType<Slots["props"]["value"]>({ id: "wrong type" });
    });

    it("returns empty object for missing slots", () => {
      type Macros = {
        props: MacroReturn<{}, "Props">;
      };

      type Slots = ExtractSlots<Macros>;
      assertType<Slots>({} as {});

      // @ts-expect-error incorrect type
      assertType<Slots["props"]["value"]>({ id: "wrong type" });
    });

    it("preserves slot function signatures", () => {
      type SlotsType = {
        default: (props: { msg: string }) => any;
        header: (props: { title: string }) => any;
        footer: () => any;
      };

      type Macros = {
        slots: MacroReturnType<SlotsType, "SlotsType">;
      };

      type Slots = ExtractSlots<Macros>;
      assertType<Slots>({} as MacroReturnType<SlotsType, "SlotsType">);
      // @ts-expect-error incorrect type
      assertType<Slots["props"]["value"]>({ id: "wrong type" });
    });

    it("handles scoped slot types", () => {
      type ScopedSlots = {
        item: (props: {
          item: { id: number; name: string };
          index: number;
        }) => any;
      };

      type Macros = {
        slots: MacroReturnObject<ScopedSlots, "ScopedSlotsObject">;
      };

      type Slots = ExtractSlots<Macros>;
      assertType<Slots>(
        {} as MacroReturnObject<ScopedSlots, "ScopedSlotsObject">
      );
      // @ts-expect-error incorrect type
      assertType<Slots["props"]["value"]>({ id: "wrong type" });
    });
  });

  describe("ExtractOptions", () => {
    it("extracts options from macro metadata", () => {
      type Macros = {
        options: MacroReturn<{ name: string }, "Options">;
      };

      type Options = ExtractOptions<Macros>;
      assertType<Options>({} as MacroReturn<{ name: string }, "Options">);

      // @ts-expect-error incorrect type
      assertType<Options["props"]["value"]>({ id: "wrong type" });
    });

    it("returns empty object for missing options", () => {
      type Macros = {
        props: MacroReturn<{}, "Props">;
      };

      type Options = ExtractOptions<Macros>;
      assertType<Options>({} as {});

      // @ts-expect-error incorrect type
      assertType<Options["props"]["value"]>({ id: "wrong type" });
    });

    it("preserves component options structure", () => {
      type ComponentOptions = {
        name: string;
        inheritAttrs: boolean;
        customOption?: any;
      };

      type Macros = {
        options: MacroReturnType<ComponentOptions, "OptionsType">;
      };

      type Options = ExtractOptions<Macros>;
      assertType<Options>(
        {} as MacroReturnType<ComponentOptions, "OptionsType">
      );

      // @ts-expect-error incorrect type
      assertType<Options["props"]["value"]>({ id: "wrong type" });
    });

    it("handles complex options with computed and methods", () => {
      type OptionsValue = {
        name: string;
        computed: Record<string, () => any>;
        methods: Record<string, (...args: any[]) => any>;
      };

      type Macros = {
        options: MacroReturnObject<OptionsValue, "ComplexOptions">;
      };

      type Options = ExtractOptions<Macros>;
      assertType<Options>(
        {} as MacroReturnObject<OptionsValue, "ComplexOptions">
      );

      // @ts-expect-error incorrect type
      assertType<Options["props"]["value"]>({ id: "wrong type" });
    });
  });

  describe("ExtractModel", () => {
    it("extracts model from macro metadata", () => {
      type Macros = {
        model: {
          value: MacroReturn<string, "string">;
        };
      };

      type Model = ExtractModel<Macros>;
      assertType<Model>({} as { value: MacroReturn<string, "string"> });
      // @ts-expect-error incorrect type
      assertType<Model["props"]["value"]>({ id: "wrong type" });
    });

    it("returns empty object for missing model", () => {
      type Macros = {
        props: MacroReturn<{}, "Props">;
      };

      type Model = ExtractModel<Macros>;
      assertType<Model>({} as {});
      // @ts-expect-error incorrect type
      assertType<Model["props"]["value"]>({ id: "wrong type" });
    });

    it("handles multiple model properties", () => {
      type Macros = {
        model: {
          firstName: MacroReturn<string, "string">;
          lastName: MacroReturn<string, "string">;
          age: MacroReturn<number, "number">;
        };
      };

      type Model = ExtractModel<Macros>;
      type Expected = {
        firstName: MacroReturn<string, "string">;
        lastName: MacroReturn<string, "string">;
        age: MacroReturn<number, "number">;
      };

      assertType<Model>({} as Expected);
      // @ts-expect-error incorrect type
      assertType<Model["props"]["value"]>({ id: "wrong type" });
    });

    it("preserves model value and type structure", () => {
      type Macros = {
        model: {
          count: MacroReturnType<number, "number">;
          title: MacroReturnObject<string, { type: "string"; required: true }>;
        };
      };

      type Model = ExtractModel<Macros>;
      type Expected = {
        count: MacroReturnType<number, "number">;
        title: MacroReturnObject<string, { type: "string"; required: true }>;
      };

      assertType<Model>({} as Expected);

      // @ts-expect-error incorrect type
      assertType<Model["props"]["value"]>({ id: "wrong type" });
    });

    it("handles empty model object", () => {
      type Macros = {
        model: {};
      };

      type Model = ExtractModel<Macros>;
      assertType<Model>({} as {});

      // @ts-expect-error incorrect type
      assertType<Model["props"]["value"]>({ id: "wrong type" });
    });

    it("handles complex model value types", () => {
      type UserModel = {
        id: number;
        name: string;
        settings: { theme: string };
      };

      type Macros = {
        model: {
          user: MacroReturn<UserModel, "UserType">;
        };
      };

      type Model = ExtractModel<Macros>;
      assertType<Model>({} as { user: MacroReturn<UserModel, "UserType"> });

      // @ts-expect-error incorrect type
      assertType<Model["props"]["value"]>({ id: "wrong type" });
    });
  });

  describe("ExtractExpose", () => {
    it("extracts expose from macro metadata", () => {
      type Macros = {
        expose: MacroReturn<{ method: () => void }, "Expose">;
      };

      type Expose = ExtractExpose<Macros>;
      assertType<Expose>({} as MacroReturn<{ method: () => void }, "Expose">);

      // @ts-expect-error incorrect type
      assertType<Expose["props"]["value"]>({ id: "wrong type" });
    });

    it("returns empty object for missing expose", () => {
      type Macros = {
        props: MacroReturn<{}, "Props">;
      };

      type Expose = ExtractExpose<Macros>;
      assertType<Expose>({} as {});
      // @ts-expect-error incorrect type
      assertType<Expose["props"]["value"]>({ id: "wrong type" });
    });

    it("preserves exposed method signatures", () => {
      type ExposedAPI = {
        focus: () => void;
        blur: () => void;
        getValue: () => string;
        setValue: (value: string) => void;
      };

      type Macros = {
        expose: MacroReturnType<ExposedAPI, "ExposeType">;
      };

      type Expose = ExtractExpose<Macros>;
      assertType<Expose>({} as MacroReturnType<ExposedAPI, "ExposeType">);
      // @ts-expect-error incorrect type
      assertType<Expose["props"]["value"]>({ id: "wrong type" });
    });

    it("handles async methods in expose", () => {
      type ExposedAPI = {
        fetchData: () => Promise<string>;
        saveData: (data: string) => Promise<void>;
      };

      type Macros = {
        expose: MacroReturnObject<ExposedAPI, "AsyncExpose">;
      };

      type Expose = ExtractExpose<Macros>;
      assertType<Expose>({} as MacroReturnObject<ExposedAPI, "AsyncExpose">);
      // @ts-expect-error incorrect type
      assertType<Expose["props"]["value"]>({ id: "wrong type" });
    });

    it("handles exposed properties and methods", () => {
      type ExposedAPI = {
        count: number;
        increment: () => void;
        decrement: () => void;
        reset: () => void;
      };

      type Macros = {
        expose: MacroReturn<ExposedAPI, "CounterExpose">;
      };

      type Expose = ExtractExpose<Macros>;
      assertType<Expose>({} as MacroReturn<ExposedAPI, "CounterExpose">);
      // @ts-expect-error incorrect type
      assertType<Expose["props"]["value"]>({ id: "wrong type" });
    });
  });

  describe("integration tests for Extract* helpers", () => {
    it("extracts all macros from complete setup", () => {
      type Macros = {
        props: MacroReturn<{ id: number }, "Props">;
        emits: MacroReturn<() => void, "Emits">;
        slots: MacroReturn<{}, "Slots">;
        options: MacroReturn<{ name: string }, "Options">;
        model: { value: MacroReturn<string, "string"> };
        expose: MacroReturn<{ focus: () => void }, "Expose">;
      };

      type Props = ExtractMacroProps<Macros>;
      type Emits = ExtractEmits<Macros>;
      type Slots = ExtractSlots<Macros>;
      type Options = ExtractOptions<Macros>;
      type Model = ExtractModel<Macros>;
      type Expose = ExtractExpose<Macros>;

      assertType<Props>(
        {} as { props: MacroReturn<{ id: number }, "Props"> } & { defaults: {} }
      );
      assertType<Emits>({} as MacroReturn<() => void, "Emits">);
      assertType<Slots>({} as MacroReturn<{}, "Slots">);
      assertType<Options>({} as MacroReturn<{ name: string }, "Options">);
      assertType<Model>({} as { value: MacroReturn<string, "string"> });
      assertType<Expose>({} as MacroReturn<{ focus: () => void }, "Expose">);

      //@ts-expect-error incorrect type
      assertType<Props["foo"]>({} as {});
      //@ts-expect-error incorrect type
      assertType<Emits["foo"]>({} as {});
      //@ts-expect-error incorrect type
      assertType<Slots["foo"]>({} as {});
      //@ts-expect-error incorrect type
      assertType<Options["foo"]>({} as {});
      //@ts-expect-error incorrect type
      assertType<Model["foo"]>({} as {});
      //@ts-expect-error incorrect type
      assertType<Expose["foo"]>({} as {});
    });

    it("extracts from partial macro metadata", () => {
      type Macros = {
        props: MacroReturn<{ id: number }, "Props">;
        emits: MacroReturn<() => void, "Emits">;
      };

      type Props = ExtractMacroProps<Macros>;
      type Emits = ExtractEmits<Macros>;
      type Slots = ExtractSlots<Macros>; // missing, should return {}
      type Options = ExtractOptions<Macros>; // missing, should return {}

      assertType<Props>(
        {} as { props: MacroReturn<{ id: number }, "Props"> } & { defaults: {} }
      );
      assertType<Emits>({} as MacroReturn<() => void, "Emits">);
      assertType<Slots>({} as {});
      assertType<Options>({} as {});

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("works with createMacroReturn result", () => {
      const setup = () =>
        createMacroReturn({
          props: {
            value: { id: 1, name: "Test" },
            type: "PropsType" as const,
          },
          emits: { value: () => {}, type: "EmitsType" as const },
          slots: { value: {}, type: "SlotsType" as const },
          model: {
            value: { value: "test", type: "string" as const },
          },
        });

      type SetupReturn = ReturnType<typeof setup>;
      type Macros = ExtractMacroReturn<SetupReturn>;

      type Props = ExtractMacroProps<Macros>;
      type Emits = ExtractEmits<Macros>;
      type Slots = ExtractSlots<Macros>;
      type Model = ExtractModel<Macros>;

      assertType<Props["props"]["value"]>({} as { id: number; name: string });
      assertType<Emits["value"]>({} as () => void);
      assertType<Slots["value"]>({} as {});
      assertType<Model["value"]["value"]>({} as string);

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("chaining ExtractMacroReturn and Extract*", () => {
      const setup = () =>
        createMacroReturn({
          props: {
            value: { count: 0, title: "Test" },
            type: "Props" as const,
          },
          expose: {
            value: { increment: () => {}, decrement: () => {} },
            type: "Expose" as const,
          },
        });

      type Macros = ExtractMacroReturn<ReturnType<typeof setup>>;
      type Props = ExtractMacroProps<Macros>;
      type Expose = ExtractExpose<Macros>;

      assertType<Props["props"]["value"]>(
        {} as { count: number; title: string }
      );
      assertType<Expose["value"]>(
        {} as { increment: () => void; decrement: () => void }
      );

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("handles complex real-world setup pattern", () => {
      type SetupReturn = {
        props: MacroReturnType<
          { modelValue: string; disabled?: boolean },
          "Props"
        >;
        emits: MacroReturnObject<
          (event: string, ...args: any[]) => void,
          { "update:modelValue": [value: string] }
        >;
        expose: MacroReturnType<
          { focus: () => void; blur: () => void },
          "Expose"
        >;
        model: {
          modelValue: MacroReturnType<string, "string">;
        };
      };

      type Props = ExtractMacroProps<SetupReturn>;
      type Emits = ExtractEmits<SetupReturn>;
      type Expose = ExtractExpose<SetupReturn>;
      type Model = ExtractModel<SetupReturn>;

      assertType<Props["props"]["value"]>(
        {} as { modelValue: string; disabled?: boolean }
      );
      assertType<Emits["object"]>(
        {} as { "update:modelValue": [value: string] }
      );
      assertType<Expose["value"]>(
        {} as { focus: () => void; blur: () => void }
      );
      assertType<Model["modelValue"]["value"]>({} as string);

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("ensures type safety with never for missing required macros", () => {
      type IncompleteMacros = {
        props: MacroReturn<{}, "Props">;
        // missing other macros
      };

      type Emits = ExtractMacro<IncompleteMacros, "emits">; // no fallback
      type Slots = ExtractMacro<IncompleteMacros, "slots">; // no fallback

      assertType<Emits>({} as never);
      assertType<Slots>({} as never);

      // @ts-expect-error incorrect type
      assertType<Emits["props"]["value"]>({ id: "wrong type" });
    });
  });

  describe("generic tests for Extract* helpers", () => {
    it("ExtractProps with generic type parameter", () => {
      function setup<T>() {
        type Props = {
          value: T;
          onChange: (value: T) => void;
        };
        const props = defineProps<Props>();

        return {
          ...createMacroReturn({
            props: {
              value: props,
              type: {} as Props,
            },
          }),
        };
      }

      type Test<T> = ReturnType<typeof setup<T>>;
      type Props<T> = ExtractMacroProps<ExtractMacroReturn<Test<T>>>;
      const foo = {} as Props<number>;

      assertType<number>(foo.props.value.value);
      assertType<(value: number) => void>(foo.props.value.onChange);

      const bar = {} as Props<string>;
      assertType<string>(bar.props.value.value);
      assertType<(value: string) => void>(bar.props.value.onChange);

      // @ts-expect-error readonly
      foo.props.value.value = "42";

      // @ts-expect-error readonly
      bar.props.value.value = "42";
    });

    it("ExtractEmits with generic event type", () => {
      function setup<T>() {
        type EmitFn = {
          (event: "change", value: T): void;
          (event: "update", data: { value: T }): void;
        };

        return createMacroReturn({
          emits: {
            value: (() => {}) as EmitFn,
            type: {} as EmitFn,
          },
        });
      }

      type Test<T> = ReturnType<typeof setup<T>>;
      type Emits<T> = ExtractEmits<ExtractMacroReturn<Test<T>>>;

      const emit = {} as Emits<number>;
      assertType<
        ((event: "change", value: number) => void) &
          ((event: "update", data: { value: number }) => void)
      >(emit.value);

      // @ts-expect-error incorrect type
      assertType<Emits["props"]["value"]>({ id: "wrong type" });
    });

    it("ExtractSlots with generic slot props", () => {
      function setup<T>() {
        type Slots = {
          default: (props: { item: T; index: number }) => any;
          empty: () => any;
        };

        return createMacroReturn({
          slots: {
            value: {} as Slots,
            type: {} as Slots,
          },
        });
      }

      type Test<T> = ReturnType<typeof setup<T>>;
      type Slots<T> = ExtractSlots<ExtractMacroReturn<Test<T>>>;

      const slots = {} as Slots<string>;
      type DefaultSlot = typeof slots.value.default;
      type SlotProps = Parameters<DefaultSlot>[0];

      assertType<string>({} as SlotProps["item"]);
      assertType<number>({} as SlotProps["index"]);

      // @ts-expect-error incorrect type
      assertType<Slots["props"]["value"]>({ id: "wrong type" });
    });

    it("ExtractModel with generic model type", () => {
      function setup<T>() {
        return createMacroReturn({
          model: {
            value: {
              value: {} as T,
              type: {} as T,
            },
            items: {
              value: {} as T[],
              type: {} as T[],
            },
          },
        });
      }

      type Test<T> = ReturnType<typeof setup<T>>;
      type Model<T> = ExtractModel<ExtractMacroReturn<Test<T>>>;

      const model = {} as Model<number>;
      assertType<number>(model.value.value);
      assertType<number[]>(model.items.value);

      const strModel = {} as Model<string>;
      assertType<string>(strModel.value.value);
      assertType<string[]>(strModel.items.value);

      // @ts-expect-error incorrect type
      assertType<Model["props"]["value"]>({ id: "wrong type" });
    });

    it("ExtractExpose with generic exposed API", () => {
      function setup<T>() {
        type ExposedAPI = {
          getValue: () => T;
          setValue: (value: T) => void;
          transform: (fn: (value: T) => T) => void;
        };

        return createMacroReturn({
          expose: {
            value: {} as ExposedAPI,
            type: {} as ExposedAPI,
          },
        });
      }

      type Test<T> = ReturnType<typeof setup<T>>;
      type Expose<T> = ExtractExpose<ExtractMacroReturn<Test<T>>>;

      const expose = {} as Expose<boolean>;
      assertType<() => boolean>(expose.value.getValue);
      assertType<(value: boolean) => void>(expose.value.setValue);
      assertType<(fn: (value: boolean) => boolean) => void>(
        expose.value.transform
      );

      // @ts-expect-error incorrect type
      assertType<Expose["props"]["value"]>({ id: "wrong type" });
    });

    it("ExtractOptions with generic computed types", () => {
      function setup<T>() {
        type Options = {
          name: string;
          data: () => { value: T };
        };

        return createMacroReturn({
          options: {
            value: {} as Options,
            type: {} as Options,
          },
        });
      }

      type Test<T> = ReturnType<typeof setup<T>>;
      type Options<T> = ExtractOptions<ExtractMacroReturn<Test<T>>>;

      const options = {} as Options<number>;
      assertType<string>(options.value.name);
      assertType<() => { value: number }>(options.value.data);

      // @ts-expect-error incorrect type
      assertType<Options["props"]["value"]>({ id: "wrong type" });
    });

    it("multiple generic parameters", () => {
      function setup<T, U>() {
        type Props = {
          value: T;
          items: U[];
          onChange: (value: T, items: U[]) => void;
        };

        return createMacroReturn({
          props: {
            value: {} as Props,
            type: {} as Props,
          },
        });
      }

      type Test<T, U> = ReturnType<typeof setup<T, U>>;
      type Props<T, U> = ExtractMacroProps<ExtractMacroReturn<Test<T, U>>>;

      const props = {} as Props<number, string>;
      assertType<number>(props.props.value.value);
      assertType<string[]>(props.props.value.items);
      assertType<(value: number, items: string[]) => void>(
        props.props.value.onChange
      );

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("generic with constraints", () => {
      function setup<T extends { id: number; name: string }>() {
        type Props = {
          item: T;
          onUpdate: (item: T) => void;
        };

        return createMacroReturn({
          props: {
            value: {} as Props,
            type: {} as Props,
          },
        });
      }

      type Test<T extends { id: number; name: string }> = ReturnType<
        typeof setup<T>
      >;
      type Props<T extends { id: number; name: string }> = ExtractMacroProps<
        ExtractMacroReturn<Test<T>>
      >;

      type User = { id: number; name: string; email: string };
      const props = {} as Props<User>;

      assertType<User>(props.props.value.item);
      assertType<number>(props.props.value.item.id);
      assertType<string>(props.props.value.item.name);
      assertType<string>(props.props.value.item.email);

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("generic with union types", () => {
      function setup<T>() {
        type Props = {
          value: T | null;
          defaultValue: T;
        };

        return createMacroReturn({
          props: {
            value: {} as Props,
            type: {} as Props,
          },
        });
      }

      type Test<T> = ReturnType<typeof setup<T>>;
      type Props<T> = ExtractMacroProps<ExtractMacroReturn<Test<T>>>;

      const props = {} as Props<number>;
      assertType<number | null>(props.props.value.value);
      assertType<number>(props.props.value.defaultValue);

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("generic with conditional types", () => {
      function setup<T>() {
        type Props = {
          value: T;
          asArray: T extends any[] ? T : T[];
        };

        return createMacroReturn({
          props: {
            value: {} as Props,
            type: {} as Props,
          },
        });
      }

      type Test<T> = ReturnType<typeof setup<T>>;
      type Props<T> = ExtractMacroProps<ExtractMacroReturn<Test<T>>>;

      const propsNumber = {} as Props<number>;
      assertType<number>(propsNumber.props.value.value);
      assertType<number[]>(propsNumber.props.value.asArray);

      const propsArray = {} as Props<string[]>;
      assertType<string[]>(propsArray.props.value.value);
      assertType<string[]>(propsArray.props.value.asArray);

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("all macros with same generic type", () => {
      function setup<T>() {
        type Props = { value: T };
        type EmitFn = (event: "change", value: T) => void;
        type Slots = { default: (props: { value: T }) => any };
        type Expose = { getValue: () => T };

        return createMacroReturn({
          props: { value: {} as Props, type: {} as Props },
          emits: { value: (() => {}) as EmitFn, type: {} as EmitFn },
          slots: { value: {} as Slots, type: {} as Slots },
          expose: { value: {} as Expose, type: {} as Expose },
          model: {
            value: { value: {} as T, type: {} as T },
          },
        });
      }

      type Test<T> = ReturnType<typeof setup<T>>;
      type Macros<T> = ExtractMacroReturn<Test<T>>;

      type Props<T> = ExtractMacroProps<Macros<T>>;
      type Emits<T> = ExtractEmits<Macros<T>>;
      type Slots<T> = ExtractSlots<Macros<T>>;
      type Expose<T> = ExtractExpose<Macros<T>>;
      type Model<T> = ExtractModel<Macros<T>>;

      const props = {} as Props<string>;
      const emits = {} as Emits<string>;
      const slots = {} as Slots<string>;
      const expose = {} as Expose<string>;
      const model = {} as Model<string>;

      assertType<string>(props.props.value.value);
      assertType<(event: "change", value: string) => void>(emits.value);
      assertType<string>(
        {} as Parameters<typeof slots.value.default>[0]["value"]
      );
      assertType<() => string>(expose.value.getValue);
      assertType<string>(model.value.value);

      // @ts-expect-error invalid accessor
      props.foo;
      // @ts-expect-error invalid accessor
      emits.foo;
      // @ts-expect-error invalid accessor
      slots.foo;
      // @ts-expect-error invalid accessor
      expose.foo;
      // @ts-expect-error invalid accessor
      model.foo;
    });

    it("nested generic types", () => {
      function setup<T>() {
        type Props = {
          data: {
            nested: {
              value: T;
              list: T[];
            };
          };
        };

        return createMacroReturn({
          props: {
            value: {} as Props,
            type: {} as Props,
          },
        });
      }

      type Test<T> = ReturnType<typeof setup<T>>;
      type Props<T> = ExtractMacroProps<ExtractMacroReturn<Test<T>>>;

      const props = {} as Props<number>;
      assertType<number>(props.props.value.data.nested.value);
      assertType<number[]>(props.props.value.data.nested.list);

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });

    it("generic with mapped types", () => {
      function setup<T extends Record<string, any>>() {
        type Props = {
          [K in keyof T]: {
            value: T[K];
            onChange: (value: T[K]) => void;
          };
        };

        return createMacroReturn({
          props: {
            value: {} as Props,
            type: {} as Props,
          },
        });
      }

      type Test<T extends Record<string, any>> = ReturnType<typeof setup<T>>;
      type Props<T extends Record<string, any>> = ExtractMacroProps<
        ExtractMacroReturn<Test<T>>
      >;

      type FormData = { name: string; age: number; active: boolean };
      const props = {} as Props<FormData>;

      assertType<string>(props.props.value.name.value);
      assertType<(value: string) => void>(props.props.value.name.onChange);
      assertType<number>(props.props.value.age.value);
      assertType<(value: number) => void>(props.props.value.age.onChange);

      // @ts-expect-error incorrect type
      assertType<Props["props"]["value"]>({ id: "wrong type" });
    });
  });
});
