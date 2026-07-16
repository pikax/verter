import { describe, expect, it } from "vitest";
import {
  assertIntrinsicAttrHoverText,
  assertIntrinsicElementHoverText,
  hasConcreteIntrinsicType,
  looksLikeOpenIntrinsicIndex,
} from "./intrinsicElementTypes";

describe("intrinsicElementTypes", () => {
  it("flags open-index any hover (invalid for Vue and Svelte equally)", () => {
    const bad = "(index) IntrinsicElements[string]: any";
    expect(looksLikeOpenIntrinsicIndex(bad)).toBe(true);
    expect(() =>
      assertIntrinsicElementHoverText(bad, "div", ["HTMLDivElement", "HTMLAttributes"]),
    ).toThrow(/open IntrinsicElements\[string\]/);
  });

  it("flags alternate open-index spellings", () => {
    expect(looksLikeOpenIntrinsicIndex("IntrinsicElements[string]: any")).toBe(true);
    expect(looksLikeOpenIntrinsicIndex("[x: string]: any")).toBe(true);
    expect(looksLikeOpenIntrinsicIndex("JSX.IntrinsicElements[string]")).toBe(true);
  });

  it("accepts closed DOM/JSX div interfaces from either framework spelling", () => {
    const jsxStyle =
      "(property) div: DetailedHTMLProps<HTMLAttributes<HTMLDivElement>, HTMLDivElement>";
    const attrsStyle = "div: HTMLDivAttributes";
    expect(looksLikeOpenIntrinsicIndex(jsxStyle)).toBe(false);
    expect(hasConcreteIntrinsicType(jsxStyle, ["HTMLDivElement", "HTMLAttributes"])).toBe(true);
    expect(() =>
      assertIntrinsicElementHoverText(jsxStyle, "div", ["HTMLDivElement", "HTMLAttributes"]),
    ).not.toThrow();
    expect(() =>
      assertIntrinsicElementHoverText(attrsStyle, "div", ["HTMLDivElement", "HTMLDivAttributes"]),
    ).not.toThrow();
  });

  it("rejects empty hover and any-only without concrete interface", () => {
    expect(() => assertIntrinsicElementHoverText("", "div", ["HTMLDivElement"])).toThrow(/empty/);
    expect(() =>
      assertIntrinsicElementHoverText("const x: any", "div", ["HTMLDivElement"]),
    ).toThrow(/degraded to any|missing concrete/);
  });

  it("attr hover rejects open index and accepts string/boolean needles", () => {
    expect(() =>
      assertIntrinsicAttrHoverText("(index) IntrinsicElements[string]: any", "a.href", ["string"]),
    ).toThrow(/open index/);
    expect(() =>
      assertIntrinsicAttrHoverText("(property) href: string", "a.href", ["string", "href"]),
    ).not.toThrow();
  });
});
