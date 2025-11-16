import { describe, it, assertType } from "vitest";
import {
  ReturnMacros,
  RegularMacros,
  MacroReturnType,
  MacroReturnObject,
  MacroReturn,
  createMacroReturn,
  ExtractMacroReturn,
} from "./setup";

describe("Setup helpers", () => {
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
      type StringMacro = MacroReturnType<string, "StringType">;
      const macro: StringMacro = {
        value: "test",
        type: "StringType",
      };

      assertType<StringMacro["value"]>({} as string);
      assertType<StringMacro["type"]>({} as "StringType");
      void macro;
    });

    it("preserves value and type generics", () => {
      type NumberMacro = MacroReturnType<number, "NumberType">;
      const macro: NumberMacro = {
        value: 42,
        type: "NumberType",
      };

      assertType<NumberMacro["value"]>({} as number);
      assertType<NumberMacro["type"]>({} as "NumberType");
      void macro;
    });

    it("works with complex value types", () => {
      type ComplexValue = { id: number; name: string };
      type ComplexMacro = MacroReturnType<ComplexValue, "ComplexType">;
      const macro: ComplexMacro = {
        value: { id: 1, name: "test" },
        type: "ComplexType",
      };

      assertType<ComplexMacro["value"]>({} as ComplexValue);
      void macro;
    });

    it('rejects wrong structure with object instead of type', () => {
      type StringMacro = MacroReturnType<string, 'StringType'>;
      const invalid: StringMacro = {
        value: 'test',
        // @ts-expect-error wrong property name
        object: 'StringType',
      };
      void invalid;
    });
  });

  describe("MacroReturnObject", () => {
    it("creates object-based macro return structure", () => {
      type StringMacro = MacroReturnObject<string, "StringObject">;
      const macro: StringMacro = {
        value: "test",
        object: "StringObject",
      };

      assertType<StringMacro["value"]>({} as string);
      assertType<StringMacro["object"]>({} as "StringObject");
      void macro;
    });

    it("preserves value and object generics", () => {
      type NumberMacro = MacroReturnObject<number, "NumberObject">;
      const macro: NumberMacro = {
        value: 42,
        object: "NumberObject",
      };

      assertType<NumberMacro["value"]>({} as number);
      assertType<NumberMacro["object"]>({} as "NumberObject");
      void macro;
    });

    it("works with complex object types", () => {
      type ComplexObject = { foo: string; bar: number };
      type ComplexMacro = MacroReturnObject<string, ComplexObject>;
      const macro: ComplexMacro = {
        value: "test",
        object: { foo: "a", bar: 1 },
      };

      assertType<ComplexMacro["object"]>({} as ComplexObject);
      void macro;
    });

    it("rejects wrong structure with type instead of object", () => {
      type StringMacro = MacroReturnObject<string, "StringObject">;
      const invalid: StringMacro = {
        value: "test",
        // @ts-expect-error wrong property name
        type: "StringObject",
      };
      void invalid;
    });
  });

  describe("MacroReturn", () => {
    it("accepts MacroReturnType structure", () => {
      type TestMacro = MacroReturn<string, "TestType">;
      const macro: TestMacro = {
        value: "test",
        type: "TestType",
      };

      assertType<TestMacro["value"]>({} as string);
      void macro;
    });

    it("accepts MacroReturnObject structure", () => {
      type TestMacro = MacroReturn<string, "TestObject">;
      const macro: TestMacro = {
        value: "test",
        object: "TestObject",
      };

      assertType<TestMacro["value"]>({} as string);
      void macro;
    });

    it("is a union of both return types", () => {
      type TestMacro = MacroReturn<number, "Test">;

      const withType: TestMacro = {
        value: 42,
        type: "Test",
      };

      const withObject: TestMacro = {
        value: 42,
        object: "Test",
      };

      void withType;
      void withObject;
    });

    it("rejects invalid structures", () => {
      type TestMacro = MacroReturn<string, "Test">;

      // @ts-expect-error missing value
      const noValue: TestMacro = {
        type: "Test",
      };

      // @ts-expect-error missing type or object
      const noTypeOrObject: TestMacro = {
        value: "test",
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

  describe("createMacroReturn", () => {
    it("creates empty macro return", () => {
      const result = createMacroReturn({});
      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted>({} as {});
      void result;
    });

    it("creates macro return with props", () => {
      const result = createMacroReturn({
        props: { value: { id: 1 }, type: "PropsType" },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["props"]["value"]>({} as { id: number });
      assertType<Extracted["props"]["type"]>({} as "PropsType");
      void result;
    });

    it("creates macro return with emits", () => {
      const result = createMacroReturn({
        emits: { value: () => {}, object: "EmitsObject" },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["emits"]["value"]>({} as () => void);
      assertType<Extracted["emits"]["object"]>({} as "EmitsObject");
      void result;
    });

    it("creates macro return with slots", () => {
      const result = createMacroReturn({
        slots: { value: {}, type: "SlotsType" },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["slots"]["value"]>({} as {});
      assertType<Extracted["slots"]["type"]>({} as "SlotsType");
      void result;
    });

    it("creates macro return with options", () => {
      const result = createMacroReturn({
        options: { value: { name: "Test" }, object: { name: "Test" } },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["options"]["value"]>({} as { name: string });
      assertType<Extracted["options"]["object"]>({} as { name: string });
      void result;
    });

    it("creates macro return with expose", () => {
      const result = createMacroReturn({
        expose: { value: { method: () => {} }, type: "ExposeType" },
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted["expose"]["value"]>(
        {} as { method: () => void }
      );
      assertType<Extracted["expose"]["type"]>({} as "ExposeType");
      void result;
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
      void result;
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
      void result;
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
      assertType<Extracted["expose"]["value"]>(
        {} as { method: () => void }
      );
      assertType<Extracted["expose"]["type"]>({} as "Expose");
      assertType<Extracted["model"]["value"]["value"]>({} as string);
      assertType<Extracted["model"]["value"]["type"]>({} as "string");
      void result;
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
      void result;
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
      void result;
    });

    it("preserves exact return type", () => {
      const input = {
        props: { value: { id: 1 }, type: "Props" as const },
        emits: { value: () => {}, object: "Emits" as const },
      };
      const result = createMacroReturn(input);

      type Extracted = ExtractMacroReturn<typeof result>;
      assertType<Extracted>({} as typeof input);
      void result;
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
      void result;
    });

    it("rejects non-regular macros at model level", () => {
      const result = createMacroReturn({
        // @ts-expect-error model cannot be a direct MacroReturn
        model: { value: "test", type: "ModelType" },
      });
      void result;
    });

    it("handles empty model object", () => {
      const result = createMacroReturn({
        model: {},
      });

      type Extracted = ExtractMacroReturn<typeof result>;
      type ModelType = Extracted["model"];
      assertType<ModelType>({} as {});
      void result;
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
      void result;
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
      void result;
    });
  });
});
