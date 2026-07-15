import { posix } from "node:path";

import { describe, expect, it } from "vitest";

import { canonicalizePath, joinCanonical, offsetToLineChar } from "../src/paths.js";

describe("canonicalizePath", () => {
  it("converts backslashes to forward slashes", () => {
    expect(canonicalizePath("C:\\Users\\me\\repo")).toBe("c:/Users/me/repo");
    // Negative: no backslash may survive.
    expect(canonicalizePath("C:\\Users\\me\\repo")).not.toContain("\\");
  });

  it("lowercases only the Windows drive letter, never the rest of the path", () => {
    expect(canonicalizePath("D:/Wt/DX-Harness")).toBe("d:/Wt/DX-Harness");
    // The drive letter is lowered; the casing of path segments is preserved.
    expect(canonicalizePath("D:/Wt/DX-Harness")).not.toBe("d:/wt/dx-harness");
  });

  it("strips the Windows extended-length prefix", () => {
    expect(canonicalizePath("\\\\?\\C:\\x\\y")).toBe("c:/x/y");
    expect(canonicalizePath("\\\\?\\UNC\\server\\share")).toBe("//server/share");
  });

  it("strips a single trailing slash but preserves filesystem roots", () => {
    expect(canonicalizePath("c:/foo/bar/")).toBe("c:/foo/bar");
    expect(canonicalizePath("c:/")).toBe("c:/");
    expect(canonicalizePath("/")).toBe("/");
  });

  it("strips EVERY trailing slash (looped), mirroring verter_span, except roots", () => {
    // verter_span::path::canonicalize_path pops trailing slashes in a loop so the
    // result is idempotent; stripping only one leaves a residual slash and a
    // second distinct canonical id for the same directory.
    expect(canonicalizePath("c:/a/b//")).toBe("c:/a/b");
    expect(canonicalizePath("/a///")).toBe("/a");
    // Discrimination: the old single-strip behaviour would stop one slash short.
    expect(canonicalizePath("c:/a/b//")).not.toBe("c:/a/b/");
    expect(canonicalizePath("/a///")).not.toBe("/a//");
    // Roots are preserved even under repeated slashes.
    expect(canonicalizePath("c://")).toBe("c:/");
    expect(canonicalizePath("///")).toBe("/");
  });

  it("is idempotent (a repeated-slash input canonicalises in one shot)", () => {
    const once = canonicalizePath("C:\\A\\B\\\\");
    expect(once).toBe("c:/A/B");
    expect(canonicalizePath(once)).toBe(once);
  });

  it("leaves an already-canonical POSIX path with no drive untouched", () => {
    expect(canonicalizePath("/home/user/repo")).toBe("/home/user/repo");
  });
});

describe("joinCanonical", () => {
  it("preserves a leading // (UNC) prefix that posix.join collapses to a single slash", () => {
    expect(joinCanonical("//server/share", "node_modules", "typescript", "lib")).toBe(
      "//server/share/node_modules/typescript/lib",
    );
    // Discrimination: bare path.posix.join collapses the UNC `//` to `/`, which
    // diverges from verter_span's canonical UNC identity on Windows.
    expect(joinCanonical("//server/share", "node_modules", "typescript", "lib")).not.toBe(
      "/server/share/node_modules/typescript/lib",
    );
    expect(posix.join("//server/share", "node_modules", "typescript", "lib")).toBe(
      "/server/share/node_modules/typescript/lib",
    );
  });

  it("joins a normal drive or POSIX base byte-for-byte like posix.join", () => {
    expect(joinCanonical("c:/repo", "node_modules", "typescript")).toBe(
      "c:/repo/node_modules/typescript",
    );
    expect(joinCanonical("/home/me/repo", "a", "b")).toBe("/home/me/repo/a/b");
    // Non-UNC bases stay identical to posix.join — no behavioural change.
    expect(joinCanonical("c:/repo", "a", "b")).toBe(posix.join("c:/repo", "a", "b"));
    expect(joinCanonical("/home/me/repo", "a", "b")).toBe(posix.join("/home/me/repo", "a", "b"));
  });

  it("normalises . and .. against a UNC base while keeping the // prefix", () => {
    expect(joinCanonical("//server/share/lib", "..", "package.json")).toBe(
      "//server/share/package.json",
    );
    // The base alone (no segments) round-trips its UNC identity.
    expect(joinCanonical("//server/share")).toBe("//server/share");
  });
});

describe("offsetToLineChar", () => {
  it("returns line 0 char 0 at the start", () => {
    expect(offsetToLineChar("abc", 0)).toEqual({ line: 0, character: 0 });
  });

  it("counts LF line breaks and the column within the line", () => {
    const text = "ab\ncde\nf";
    // 'd' is index 4 → line 1, char 1.
    expect(offsetToLineChar(text, 4)).toEqual({ line: 1, character: 1 });
    // 'f' is index 7 → line 2, char 0.
    expect(offsetToLineChar(text, 7)).toEqual({ line: 2, character: 0 });
  });

  it("treats CRLF as one break and yields the SAME position as the LF form", () => {
    const lf = "ab\ncd";
    const crlf = "ab\r\ncd";
    // 'c' after the break: LF index 3, CRLF index 4 — both are line 1 char 0.
    expect(offsetToLineChar(lf, 3)).toEqual({ line: 1, character: 0 });
    expect(offsetToLineChar(crlf, 4)).toEqual({ line: 1, character: 0 });
    // Discrimination: a naive byte/char count that didn't fold CRLF would put
    // the CRLF position at char 1, never char 0.
    expect(offsetToLineChar(crlf, 4).character).not.toBe(1);
  });

  it("counts characters in UTF-16 code units (the LSP column unit)", () => {
    // 'é' is one UTF-16 unit; 'x' sits at index 4 on the same line.
    const text = "café x";
    expect(offsetToLineChar(text, 5)).toEqual({ line: 0, character: 5 });
    // A surrogate-pair emoji is two UTF-16 units, so a following char advances
    // the column by two, not one.
    const emoji = "a😀b";
    expect(offsetToLineChar(emoji, 3)).toEqual({ line: 0, character: 3 });
  });

  it("clamps an out-of-range index to the text length", () => {
    const text = "ab\ncd";
    expect(offsetToLineChar(text, 999)).toEqual({ line: 1, character: 2 });
  });
});
