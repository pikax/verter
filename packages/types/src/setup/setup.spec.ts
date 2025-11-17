import { describe, it, assertType } from "vitest";
import {
  ReturnMacros,
  RegularMacros,
  MacroReturnType,
  MacroReturnObject,
  MacroReturn,
  createMacroReturn,
  ExtractMacroReturn,
  ExtractMacro,
  ExtractProps,
  ExtractEmits,
  ExtractSlots,
  ExtractOptions,
  ExtractModel,
  ExtractExpose,
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
    });

    it("returns never for missing macro without fallback", () => {
      type Macros = {
        props: MacroReturn<{ id: number }, "Props">;
      };

      type Emits = ExtractMacro<Macros, "emits">;
      assertType<Emits>({} as never);
    });

    it("returns fallback type for missing macro", () => {
      type Macros = {
        props: MacroReturn<{ id: number }, "Props">;
      };

      type Emits = ExtractMacro<Macros, "emits", () => void>;
      assertType<Emits>({} as () => void);

      type Slots = ExtractMacro<Macros, "slots", {}>;
      assertType<Slots>({} as {});
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
    });

    it("handles empty metadata object", () => {
      type Macros = {};

      type Props = ExtractMacro<Macros, "props", {}>;
      assertType<Props>({} as {});
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
    });
  });

  describe("ExtractProps", () => {
    it("extracts props from macro metadata", () => {
      type Macros = {
        props: MacroReturn<{ id: number; name: string }, "Props">;
      };

      type Props = ExtractProps<Macros>;
      assertType<Props>({} as MacroReturn<{ id: number; name: string }, "Props">);
    });

    it("returns empty object for missing props", () => {
      type Macros = {
        emits: MacroReturn<() => void, "Emits">;
      };

      type Props = ExtractProps<Macros>;
      assertType<Props>({} as {});
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

      type Props = ExtractProps<Macros>;
      assertType<Props>({} as MacroReturnType<ComplexProps, "ComplexPropsType">);
    });

    it("preserves MacroReturnType vs MacroReturnObject", () => {
      type Macros1 = {
        props: MacroReturnType<{ id: number }, "PropsType">;
      };
      type Macros2 = {
        props: MacroReturnObject<{ id: number }, { id: "number" }>;
      };

      type Props1 = ExtractProps<Macros1>;
      type Props2 = ExtractProps<Macros2>;

      assertType<Props1>({} as MacroReturnType<{ id: number }, "PropsType">);
      assertType<Props2>({} as MacroReturnObject<{ id: number }, { id: "number" }>);
    });

    it("works with empty metadata", () => {
      type Macros = {};
      type Props = ExtractProps<Macros>;
      assertType<Props>({} as {});
    });
  });

  describe("ExtractEmits", () => {
    it("extracts emits from macro metadata", () => {
      type Macros = {
        emits: MacroReturn<(event: string) => void, "Emits">;
      };

      type Emits = ExtractEmits<Macros>;
      assertType<Emits>({} as MacroReturn<(event: string) => void, "Emits">);
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
    });
  });

  describe("ExtractSlots", () => {
    it("extracts slots from macro metadata", () => {
      type Macros = {
        slots: MacroReturn<{}, "Slots">;
      };

      type Slots = ExtractSlots<Macros>;
      assertType<Slots>({} as MacroReturn<{}, "Slots">);
    });

    it("returns empty object for missing slots", () => {
      type Macros = {
        props: MacroReturn<{}, "Props">;
      };

      type Slots = ExtractSlots<Macros>;
      assertType<Slots>({} as {});
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
      assertType<Slots>({} as MacroReturnObject<ScopedSlots, "ScopedSlotsObject">);
    });
  });

  describe("ExtractOptions", () => {
    it("extracts options from macro metadata", () => {
      type Macros = {
        options: MacroReturn<{ name: string }, "Options">;
      };

      type Options = ExtractOptions<Macros>;
      assertType<Options>({} as MacroReturn<{ name: string }, "Options">);
    });

    it("returns empty object for missing options", () => {
      type Macros = {
        props: MacroReturn<{}, "Props">;
      };

      type Options = ExtractOptions<Macros>;
      assertType<Options>({} as {});
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
      assertType<Options>({} as MacroReturnType<ComponentOptions, "OptionsType">);
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
      assertType<Options>({} as MacroReturnObject<OptionsValue, "ComplexOptions">);
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
    });

    it("returns empty object for missing model", () => {
      type Macros = {
        props: MacroReturn<{}, "Props">;
      };

      type Model = ExtractModel<Macros>;
      assertType<Model>({} as {});
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
    });

    it("handles empty model object", () => {
      type Macros = {
        model: {};
      };

      type Model = ExtractModel<Macros>;
      assertType<Model>({} as {});
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
    });
  });

  describe("ExtractExpose", () => {
    it("extracts expose from macro metadata", () => {
      type Macros = {
        expose: MacroReturn<{ method: () => void }, "Expose">;
      };

      type Expose = ExtractExpose<Macros>;
      assertType<Expose>({} as MacroReturn<{ method: () => void }, "Expose">);
    });

    it("returns empty object for missing expose", () => {
      type Macros = {
        props: MacroReturn<{}, "Props">;
      };

      type Expose = ExtractExpose<Macros>;
      assertType<Expose>({} as {});
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

      type Props = ExtractProps<Macros>;
      type Emits = ExtractEmits<Macros>;
      type Slots = ExtractSlots<Macros>;
      type Options = ExtractOptions<Macros>;
      type Model = ExtractModel<Macros>;
      type Expose = ExtractExpose<Macros>;

      assertType<Props>({} as MacroReturn<{ id: number }, "Props">);
      assertType<Emits>({} as MacroReturn<() => void, "Emits">);
      assertType<Slots>({} as MacroReturn<{}, "Slots">);
      assertType<Options>({} as MacroReturn<{ name: string }, "Options">);
      assertType<Model>({} as { value: MacroReturn<string, "string"> });
      assertType<Expose>({} as MacroReturn<{ focus: () => void }, "Expose">);
    });

    it("extracts from partial macro metadata", () => {
      type Macros = {
        props: MacroReturn<{ id: number }, "Props">;
        emits: MacroReturn<() => void, "Emits">;
      };

      type Props = ExtractProps<Macros>;
      type Emits = ExtractEmits<Macros>;
      type Slots = ExtractSlots<Macros>; // missing, should return {}
      type Options = ExtractOptions<Macros>; // missing, should return {}

      assertType<Props>({} as MacroReturn<{ id: number }, "Props">);
      assertType<Emits>({} as MacroReturn<() => void, "Emits">);
      assertType<Slots>({} as {});
      assertType<Options>({} as {});
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

      type Props = ExtractProps<Macros>;
      type Emits = ExtractEmits<Macros>;
      type Slots = ExtractSlots<Macros>;
      type Model = ExtractModel<Macros>;

      assertType<Props["value"]>({} as { id: number; name: string });
      assertType<Emits["value"]>({} as () => void);
      assertType<Slots["value"]>({} as {});
      assertType<Model["value"]["value"]>({} as string);
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
      type Props = ExtractProps<Macros>;
      type Expose = ExtractExpose<Macros>;

      assertType<Props["value"]>({} as { count: number; title: string });
      assertType<Expose["value"]>(
        {} as { increment: () => void; decrement: () => void }
      );
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

      type Props = ExtractProps<SetupReturn>;
      type Emits = ExtractEmits<SetupReturn>;
      type Expose = ExtractExpose<SetupReturn>;
      type Model = ExtractModel<SetupReturn>;

      assertType<Props["value"]>(
        {} as { modelValue: string; disabled?: boolean }
      );
      assertType<Emits["object"]>(
        {} as { "update:modelValue": [value: string] }
      );
      assertType<Expose["value"]>(
        {} as { focus: () => void; blur: () => void }
      );
      assertType<Model["modelValue"]["value"]>({} as string);
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
      type Props<T> = ExtractProps<ExtractMacroReturn<Test<T>>>;
      const foo = {} as Props<number>;

      assertType<number>(foo.value.value);
      assertType<(value: number) => void>(foo.value.onChange);
      
      const bar = {} as Props<string>;
      assertType<string>(bar.value.value);
      assertType<(value: string) => void>(bar.value.onChange);
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
      assertType<((event: "change", value: number) => void) & ((event: "update", data: { value: number }) => void)>(emit.value);
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
      assertType<(fn: (value: boolean) => boolean) => void>(expose.value.transform);
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
      type Props<T, U> = ExtractProps<ExtractMacroReturn<Test<T, U>>>;
      
      const props = {} as Props<number, string>;
      assertType<number>(props.value.value);
      assertType<string[]>(props.value.items);
      assertType<(value: number, items: string[]) => void>(props.value.onChange);
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

      type Test<T extends { id: number; name: string }> = ReturnType<typeof setup<T>>;
      type Props<T extends { id: number; name: string }> = ExtractProps<ExtractMacroReturn<Test<T>>>;
      
      type User = { id: number; name: string; email: string };
      const props = {} as Props<User>;
      
      assertType<User>(props.value.item);
      assertType<number>(props.value.item.id);
      assertType<string>(props.value.item.name);
      assertType<string>(props.value.item.email);
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
      type Props<T> = ExtractProps<ExtractMacroReturn<Test<T>>>;
      
      const props = {} as Props<number>;
      assertType<number | null>(props.value.value);
      assertType<number>(props.value.defaultValue);
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
      type Props<T> = ExtractProps<ExtractMacroReturn<Test<T>>>;
      
      const propsNumber = {} as Props<number>;
      assertType<number>(propsNumber.value.value);
      assertType<number[]>(propsNumber.value.asArray);
      
      const propsArray = {} as Props<string[]>;
      assertType<string[]>(propsArray.value.value);
      assertType<string[]>(propsArray.value.asArray);
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
      
      type Props<T> = ExtractProps<Macros<T>>;
      type Emits<T> = ExtractEmits<Macros<T>>;
      type Slots<T> = ExtractSlots<Macros<T>>;
      type Expose<T> = ExtractExpose<Macros<T>>;
      type Model<T> = ExtractModel<Macros<T>>;
      
      const props = {} as Props<string>;
      const emits = {} as Emits<string>;
      const slots = {} as Slots<string>;
      const expose = {} as Expose<string>;
      const model = {} as Model<string>;
      
      assertType<string>(props.value.value);
      assertType<(event: "change", value: string) => void>(emits.value);
      assertType<string>({} as Parameters<typeof slots.value.default>[0]["value"]);
      assertType<() => string>(expose.value.getValue);
      assertType<string>(model.value.value);
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
      type Props<T> = ExtractProps<ExtractMacroReturn<Test<T>>>;
      
      const props = {} as Props<number>;
      assertType<number>(props.value.data.nested.value);
      assertType<number[]>(props.value.data.nested.list);
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
      type Props<T extends Record<string, any>> = ExtractProps<ExtractMacroReturn<Test<T>>>;
      
      type FormData = { name: string; age: number; active: boolean };
      const props = {} as Props<FormData>;
      
      assertType<string>(props.value.name.value);
      assertType<(value: string) => void>(props.value.name.onChange);
      assertType<number>(props.value.age.value);
      assertType<(value: number) => void>(props.value.age.onChange);
    });
  });
});
