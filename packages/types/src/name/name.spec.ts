/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for string case conversion type helpers including:
 * - PascalToKebab, CamelToKebab: component name transformations
 * - CanCapitalize: validates capitalizable strings
 * - Used for Vue component name resolution
 */
import { describe, it, assertType } from "vitest";
import type {
  CanCapitalize,
  PascalToKebab,
  CamelToKebab,
  NormalisePascal,
  Hyphenate,
  Camelize,
} from "./name";

describe("name helpers", () => {
  it("CanCapitalize: special chars map to 'x'", () => {
    type Dash = CanCapitalize<"-">;
    type Under = CanCapitalize<"_">;
    type Dollar = CanCapitalize<"$">;
    type Empty = CanCapitalize<"">;
    assertType<"x">({} as Dash);
    assertType<"x">({} as Under);
    assertType<"x">({} as Dollar);
    assertType<"x">({} as Empty);
  });

  it("CanCapitalize: letters capitalize correctly", () => {
    type Lower = CanCapitalize<"a">;
    type Upper = CanCapitalize<"A">;
    assertType<"A">({} as Lower);
    assertType<"A">({} as Upper);
  });

  it("PascalToKebab: base and simple pairs", () => {
    assertType<PascalToKebab<"">>({} as "");
    assertType<PascalToKebab<"A">>({} as "A");
    assertType<PascalToKebab<"a">>({} as "a");
    assertType<PascalToKebab<"AB">>({} as "A-B");
    assertType<PascalToKebab<"Aa">>({} as "Aa");
    assertType<PascalToKebab<"aA">>({} as "a-A");
  });

  it("CamelToKebab: typical camelCase", () => {
    type R = CamelToKebab<"myName">;
    // union of unchanged and lowercased tail versions
    assertType<R>({} as "myName" | "myname");
    assertType<"myName" | "myname">({} as R);
  });

  it("CamelToKebab: with acronym sequence", () => {
    type R = CamelToKebab<"myHTTPServer">;
    type Expected = "myHTTPServer" | "myH-TT-PServer" | "myh-tt-pserver";
    assertType<R>({} as Expected);
    assertType<Expected>({} as R);
  });

  it("CamelToKebab: leading uppercase coerces first letter to lowercase", () => {
    type R = CamelToKebab<"My">;
    assertType<R>({} as "my");
    assertType<"my">({} as R);
  });

  it("NormalisePascal: uppercase input normalised to hyphenated variants", () => {
    type R = NormalisePascal<"MyName">;
    type Expected = "MyName" | "myname";
    assertType<R>({} as Expected);
    assertType<Expected>({} as R);
  });

  it("NormalisePascal: lowercase input stays as-is", () => {
    type R = NormalisePascal<"myName">;
    assertType<R>({} as "myName");
    assertType<"myName">({} as R);
  });

  // Vue parity tests
  it("Hyphenate (vue/core): camelCase to kebab-case lowercase", () => {
    type H1 = Hyphenate<"myName">;
    assertType<H1>({} as "my-name");
    assertType<"my-name">({} as H1);

    type H2 = Hyphenate<"MyName">;
    assertType<H2>({} as "my-name");
    assertType<"my-name">({} as H2);

    type H3 = Hyphenate<"myHTTPServer">;
    assertType<H3>({} as "my-h-t-t-p-server");
    assertType<"my-h-t-t-p-server">({} as H3);

    type H4 = Hyphenate<"already-kebab">;
    assertType<H4>({} as "already-kebab");
    assertType<"already-kebab">({} as H4);

    type H5 = Hyphenate<"A">;
    assertType<H5>({} as "a");
    assertType<"a">({} as H5);

    type H6 = Hyphenate<"">;
    assertType<H6>({} as "");
    assertType<"">({} as H6);
  });

  it("Hyphenate: distributes over unions", () => {
    type R = Hyphenate<"MyName" | "FooBar">;
    assertType<R>({} as "my-name" | "foo-bar");
    assertType<"my-name" | "foo-bar">({} as R);
  });

  it("Camelize (vue/core): kebab-case to camelCase", () => {
    type C1 = Camelize<"my-name">;
    assertType<C1>({} as "myName");
    assertType<"myName">({} as C1);

    type C2 = Camelize<"my-long-name">;
    assertType<C2>({} as "myLongName");
    assertType<"myLongName">({} as C2);

    type C3 = Camelize<"alreadyCamel">;
    assertType<C3>({} as "alreadyCamel");
    assertType<"alreadyCamel">({} as C3);

    type C4 = Camelize<"a">;
    assertType<C4>({} as "a");
    assertType<"a">({} as C4);

    type C5 = Camelize<"">;
    assertType<C5>({} as "");
    assertType<"">({} as C5);
  });
});
