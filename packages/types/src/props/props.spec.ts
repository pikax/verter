/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for defineProps type helpers including:
 * - PropsWithDefaults: tracks which props have defaults
 * - MakePublicProps/MakeInternalProps: handles optional vs required props
 * - Validates withDefaults integration and prop type inference
 */
import { describe, assertType, it } from "vitest";
import type { DefineProps } from "vue";
import type {
  PropsWithDefaults,
  MakePublicProps,
  MakeBooleanOptional,
  ExtractBooleanKeys,
  MakeInternalProps,
} from "./props";

describe("PropsWithDefaults", () => {
  it("should mark props with defaults", () => {
    type Props = {
      name: string;
      age: number;
      active: boolean;
    };

    type WithDefaults = PropsWithDefaults<Props, "name" | "active">;

    // The type should still have all the original properties
    assertType<WithDefaults>({} as Props);
  });

  it("should work with optional props", () => {
    type Props = {
      name?: string;
      age: number;
    };

    type WithDefaults = PropsWithDefaults<Props, "name">;

    assertType<WithDefaults>({} as Props);
  });
});

describe("MakePublicProps", () => {
  it("should make props with defaults optional in public API", () => {
    type Props = {
      readonly id: number;
      readonly name: string;
    };

    type PropsWithDef = PropsWithDefaults<Props, "name">;
    type PublicProps = MakePublicProps<PropsWithDef>;

    // id has no default, so it's required
    assertType<number>({} as PublicProps["id"]);

    // name has a default, so it's optional and can be undefined
    assertType<string | undefined>({} as PublicProps["name"]);

    // Test that name prop can accept undefined
    const testPublicProps: PublicProps = { id: 1, name: undefined };
    assertType<PublicProps>(testPublicProps);
  });

  it("should handle props without defaults marker", () => {
    type Props = {
      readonly name: string;
      readonly age: number;
    };

    type PublicProps = MakePublicProps<Props>;

    // Without PropsWithDefaults marker, MakeBooleanOptional adds | undefined to all props
    assertType<string | undefined>({} as PublicProps["name"]);
    assertType<number | undefined>({} as PublicProps["age"]);
  });

  it("should handle all props with defaults", () => {
    type Props = {
      readonly name: string;
      readonly age: number;
      readonly active: boolean;
    };

    type PropsWithDef = PropsWithDefaults<Props, "name" | "age" | "active">;
    type PublicProps = MakePublicProps<PropsWithDef>;

    // All props have defaults, so all should be optional
    assertType<string | undefined>({} as PublicProps["name"]);
    assertType<number | undefined>({} as PublicProps["age"]);
    assertType<boolean | undefined>({} as PublicProps["active"]);
  });

  it("should preserve readonly modifier", () => {
    type Props = {
      readonly id: number;
      readonly name: string;
    };

    type PropsWithDef = PropsWithDefaults<Props, "name">;
    type PublicProps = MakePublicProps<PropsWithDef>;

    const testReadonly: PublicProps = { id: 1, name: "test" };
    assertType<PublicProps>(testReadonly);
  });

  it("should handle mixed optional and required props", () => {
    type Props = {
      readonly required: string;
      readonly optional?: number;
      readonly withDefault: boolean;
    };

    type PropsWithDef = PropsWithDefaults<Props, "withDefault">;
    type PublicProps = MakePublicProps<PropsWithDef>;

    assertType<string>({} as PublicProps["required"]);
    assertType<number | undefined>({} as PublicProps["optional"]);
    assertType<boolean | undefined>({} as PublicProps["withDefault"]);
  });
});

describe("MakeBooleanOptional", () => {
  it("should make boolean props optional for DefineProps", () => {
    type Props = DefineProps<
      {
        name: string;
        active: boolean;
        visible: boolean;
      },
      "active" | "visible"
    >;

    type Result = MakeBooleanOptional<Props>;

    // name should remain required
    assertType<string>({} as Result["name"]);

    // Boolean props should be optional
    assertType<boolean | undefined>({} as Result["active"]);
    assertType<boolean | undefined>({} as Result["visible"]);
  });

  it("should handle DefineProps with no boolean keys", () => {
    type Props = DefineProps<
      {
        name: string;
        age: number;
      },
      never
    >;

    type Result = MakeBooleanOptional<Props>;

    assertType<string>({} as Result["name"]);
    assertType<number>({} as Result["age"]);
  });

  it("should return unchanged type if not DefineProps", () => {
    type Props = {
      name: string;
      active: boolean;
    };

    type Result = MakeBooleanOptional<Props>;

    assertType<Props>({} as Result);
  });

  it("should handle single boolean key", () => {
    type Props = DefineProps<
      {
        name: string;
        active: boolean;
      },
      "active"
    >;

    type Result = MakeBooleanOptional<Props>;

    assertType<string>({} as Result["name"]);
    assertType<boolean | undefined>({} as Result["active"]);
  });
});

describe("ExtractBooleanKeys", () => {
  it("should extract boolean keys from DefineProps", () => {
    type Props = DefineProps<
      {
        name: string;
        active: boolean;
        visible: boolean;
      },
      "active" | "visible"
    >;

    type Keys = ExtractBooleanKeys<Props>;

    assertType<"active" | "visible">({} as Keys);
  });

  it("should return never if no boolean keys", () => {
    type Props = DefineProps<
      {
        name: string;
        age: number;
      },
      never
    >;

    type Keys = ExtractBooleanKeys<Props>;

    assertType<never>({} as Keys);
  });

  it("should return never if not DefineProps", () => {
    type Props = {
      name: string;
      active: boolean;
    };

    type Keys = ExtractBooleanKeys<Props>;

    assertType<never>({} as Keys);
  });

  it("should handle single boolean key", () => {
    type Props = DefineProps<
      {
        name: string;
        active: boolean;
      },
      "active"
    >;

    type Keys = ExtractBooleanKeys<Props>;

    assertType<"active">({} as Keys);
  });
});

describe("MakeInternalProps", () => {
  it("should make props with defaults required internally", () => {
    type Props = {
      readonly id: number;
      readonly name: string;
    };

    type PropsWithDef = PropsWithDefaults<Props, "name">;
    type InternalProps = MakeInternalProps<PropsWithDef>;

    // Both props should be required internally
    assertType<number>({} as InternalProps["id"]);
    assertType<string | undefined>({} as InternalProps["name"]);

    // name should be required (not optional), even though it can be undefined
    type NameIsRequired = undefined extends InternalProps["name"]
      ? "can-be-undefined"
      : "required-only";
    assertType<"can-be-undefined">({} as NameIsRequired);
  });

  it("should handle props without defaults marker", () => {
    type Props = {
      readonly name: string;
      readonly age: number;
    };

    type InternalProps = MakeInternalProps<Props>;

    // Without PropsWithDefaults marker, MakeBooleanOptional makes all props have | undefined
    assertType<string | undefined>({} as InternalProps["name"]);
    assertType<number | undefined>({} as InternalProps["age"]);
  });

  it("should handle all props with defaults", () => {
    type Props = {
      readonly name: string;
      readonly age: number;
      readonly active: boolean;
    };

    type PropsWithDef = PropsWithDefaults<Props, "name" | "age" | "active">;
    type InternalProps = MakeInternalProps<PropsWithDef>;

    // All props are required internally but can be undefined
    assertType<string | undefined>({} as InternalProps["name"]);
    assertType<number | undefined>({} as InternalProps["age"]);
    assertType<boolean | undefined>({} as InternalProps["active"]);
  });

  it("should preserve readonly modifier", () => {
    type Props = {
      readonly id: number;
      readonly name: string;
    };

    type PropsWithDef = PropsWithDefaults<Props, "name">;
    type InternalProps = MakeInternalProps<PropsWithDef>;

    const testReadonly: InternalProps = { id: 1, name: "test" };
    assertType<InternalProps>(testReadonly);
  });

  it("should make all default props required but possibly undefined", () => {
    type Props = {
      readonly count: number;
      readonly label?: string;
    };

    type PropsWithDef = PropsWithDefaults<Props, "count">;
    type InternalProps = MakeInternalProps<PropsWithDef>;

    // count has default so it's required but can be undefined
    assertType<number | undefined>({} as InternalProps["count"]);

    // label has no default
    assertType<string | undefined>({} as InternalProps["label"]);
  });
});

describe("Integration tests", () => {
  it("should correctly transform props from public to internal API", () => {
    type Props = {
      readonly id: number;
      readonly name: string;
      readonly count: number;
    };

    // Props with defaults: name and count
    type PropsWithDef = PropsWithDefaults<Props, "name" | "count">;

    // Public API: defaults are optional
    type PublicProps = MakePublicProps<PropsWithDef>;

    // Internal API: defaults are required
    type InternalProps = MakeInternalProps<PropsWithDef>;

    // Public: name and count are optional
    assertType<number>({} as PublicProps["id"]);
    assertType<string | undefined>({} as PublicProps["name"]);
    assertType<number | undefined>({} as PublicProps["count"]);

    // Internal: all required, but defaults can be undefined
    assertType<number>({} as InternalProps["id"]);
    assertType<string | undefined>({} as InternalProps["name"]);
    assertType<number | undefined>({} as InternalProps["count"]);
  });

  it("should handle complex prop scenarios", () => {
    type Props = {
      readonly required: string;
      readonly optional?: number;
      readonly withDefault: boolean;
      readonly anotherDefault: string;
    };

    type PropsWithDef = PropsWithDefaults<Props, "withDefault" | "anotherDefault">;

    type PublicProps = MakePublicProps<PropsWithDef>;
    type InternalProps = MakeInternalProps<PropsWithDef>;

    // Public API
    assertType<string>({} as PublicProps["required"]);
    assertType<number | undefined>({} as PublicProps["optional"]);
    assertType<boolean | undefined>({} as PublicProps["withDefault"]);
    assertType<string | undefined>({} as PublicProps["anotherDefault"]);

    // Internal API
    assertType<string>({} as InternalProps["required"]);
    assertType<number | undefined>({} as InternalProps["optional"]);
    assertType<boolean | undefined>({} as InternalProps["withDefault"]);
    assertType<string | undefined>({} as InternalProps["anotherDefault"]);
  });

  it("should work with DefineProps and boolean keys", () => {
    type Props = DefineProps<
      {
        name: string;
        active: boolean;
        visible: boolean;
        count: number;
      },
      "active" | "visible"
    >;

    type PropsWithDef = PropsWithDefaults<Props, "count">;
    type PublicProps = MakePublicProps<PropsWithDef>;

    // Boolean props are optional
    assertType<boolean | undefined>({} as PublicProps["active"]);
    assertType<boolean | undefined>({} as PublicProps["visible"]);

    // Prop with default is optional
    assertType<number | undefined>({} as PublicProps["count"]);

    // Regular prop without default
    assertType<string>({} as PublicProps["name"]);
  });
});
