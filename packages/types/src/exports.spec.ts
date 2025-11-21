// This file verifies that all types are properly exported from the package
import { describe, it, assertType } from "vitest";

// Helpers
import type {
  PatchHidden,
  ExtractHidden,
  FunctionToObject,
  IntersectionFunctionToObject,
  PartialUndefined,
  UnionToIntersection,
} from "./index";

// Emits
import type { EmitsToProps, ComponentEmitsToProps } from "./index";

// Model
import type { ModelToEmits, ModelToProps, ModelToModelInfo } from "./index";

// Components
import type { GetVueComponent, DefineOptions } from "./index";

// Slots
import type { StrictRenderSlot } from "./index";

// Props
import type {
  PropsWithDefaults,
  MakePublicProps,
  MakeBooleanOptional,
  ExtractBooleanKeys,
  MakeInternalProps,
} from "./index";

// Setup
import type {
  ReturnMacros,
  RegularMacros,
  MacroReturnType,
  MacroReturnObject,
  MacroReturn,
  CreateMacroReturn,
  OmitMacroReturn,
  ExtractMacroReturn,
  ExtractMacro,
  ExtractTemplateRef,
  ExtractMacroProps,
  ExtractPropsFromMacro,
  ExtractEmits,
  MacroToEmitValue,
  ExtractSlots,
  SlotsToSlotType,
  ExtractOptions,
  MacroOptionsToOptions,
  ExtractModel,
  MacroToModelType,
  MacroToModelRecord,
  ExtractExpose,
  ExposeToVueExposeKey,
  NormaliseMacroReturn,
  NormalisedMacroReturn,
} from "./index";

import { createMacroReturn } from "./index";

// Name
import type {
  CanCapitalize,
  PascalToKebab,
  CamelToKebab,
  NormalisePascal,
  Hyphenate,
  Camelize,
} from "./index";

// Instance
import type {
  CreateTypedInternalInstanceFromNormalisedMacro,
  ToInstanceProps,
  CreateTypedPublicInstanceFromNormalisedMacro,
  PublicInstanceFromMacro,
  PublicInstanceFromNormalisedMacro,
  CreateExportedInstanceFromNormalisedMacro,
  CreateExportedInstanceFromMacro,
} from "./index";

describe("Package exports", () => {
  it("all types are properly exported", () => {
    // This test verifies that all imports compile successfully
    // If any type is missing from exports, TypeScript will error

    // Helper types
    type TestPatchHidden = PatchHidden<{ a: string }, { b: number }>;
    type TestExtractHidden = ExtractHidden<TestPatchHidden>;
    type TestFunctionToObject = FunctionToObject<(e: "test", val: number) => void>;
    type TestIntersectionFunctionToObject = IntersectionFunctionToObject<
      ((e: "foo") => void) & ((e: "bar") => void)
    >;
    type TestPartialUndefined = PartialUndefined<{ a: string; b?: number }>;
    type TestUnionToIntersection = UnionToIntersection<{ a: string } | { b: number }>;

    // Emit types
    type TestEmitsToProps = EmitsToProps<(e: "update", val: number) => void>;
    type TestComponentEmitsToProps = ComponentEmitsToProps<any>;

    // Model types
    type TestModelToEmits = ModelToEmits<{}>;
    type TestModelToProps = ModelToProps<{}>;
    type TestModelToModelInfo = ModelToModelInfo<{}>;

    // Component types
    type TestGetVueComponent = GetVueComponent<any>;

    // Props types
    type TestPropsWithDefaults = PropsWithDefaults<{ a: string }, "a">;
    type TestMakePublicProps = MakePublicProps<TestPropsWithDefaults>;
    type TestMakeBooleanOptional = MakeBooleanOptional<any>;
    type TestExtractBooleanKeys = ExtractBooleanKeys<any>;
    type TestMakeInternalProps = MakeInternalProps<TestPropsWithDefaults>;

    // Setup types
    type TestReturnMacros = ReturnMacros;
    type TestRegularMacros = RegularMacros;
    type TestMacroReturnType = MacroReturnType<any, any>;
    type TestMacroReturnObject = MacroReturnObject<any, any>;
    type TestMacroReturn = MacroReturn<any, any>;
    type TestCreateMacroReturn = CreateMacroReturn<any>;
    type TestOmitMacroReturn = OmitMacroReturn<any>;
    type TestExtractMacroReturn = ExtractMacroReturn<any>;
    type TestExtractMacro = ExtractMacro<any, "props">;
    type TestExtractTemplateRef = ExtractTemplateRef<any>;
    type TestExtractMacroProps = ExtractMacroProps<any>;
    type TestExtractPropsFromMacro = ExtractPropsFromMacro<any>;
    type TestExtractEmits = ExtractEmits<any>;
    type TestMacroToEmitValue = MacroToEmitValue<any>;
    type TestExtractSlots = ExtractSlots<any>;
    type TestSlotsToSlotType = SlotsToSlotType<any>;
    type TestExtractOptions = ExtractOptions<any>;
    type TestMacroOptionsToOptions = MacroOptionsToOptions<any>;
    type TestExtractModel = ExtractModel<any>;
    type TestMacroToModelType = MacroToModelType<any>;
    type TestMacroToModelRecord = MacroToModelRecord<any>;
    type TestExtractExpose = ExtractExpose<any>;
    type TestExposeToVueExposeKey = ExposeToVueExposeKey<any>;
    type TestNormaliseMacroReturn = NormaliseMacroReturn<any>;
    type TestNormalisedMacroReturn = NormalisedMacroReturn<any>;

    // Name types
    type TestCanCapitalize = CanCapitalize<"a">;
    type TestPascalToKebab = PascalToKebab<"MyName">;
    type TestCamelToKebab = CamelToKebab<"myName">;
    type TestNormalisePascal = NormalisePascal<"MyName">;
    type TestHyphenate = Hyphenate<"MyName">;
    type TestCamelize = Camelize<"my-name">;

    // Instance types
    type TestCreateTypedInternalInstanceFromNormalisedMacro =
      CreateTypedInternalInstanceFromNormalisedMacro<any>;
    type TestToInstanceProps = ToInstanceProps<any, true>;
    type TestCreateTypedPublicInstanceFromNormalisedMacro =
      CreateTypedPublicInstanceFromNormalisedMacro<any>;
    type TestPublicInstanceFromMacro = PublicInstanceFromMacro<any, any, any, any, any>;
    type TestPublicInstanceFromNormalisedMacro = PublicInstanceFromNormalisedMacro<any, any, any, any, any>;
    type TestCreateExportedInstanceFromNormalisedMacro =
      CreateExportedInstanceFromNormalisedMacro<any>;
    type TestCreateExportedInstanceFromMacro = CreateExportedInstanceFromMacro<any>;

    // Verify function exports
    const testCreateMacroReturn = createMacroReturn({});
    assertType<CreateMacroReturn<{}>>(testCreateMacroReturn);

    // If we got here, all types are properly exported
    assertType<true>(true);
  });
});
