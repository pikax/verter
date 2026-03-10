const DEFAULT_REGEXP = /\.vue$/;
const VUE_TS_REGEXP = /\.vue\.ts$/;
const VUE_D_TS_REGEXP = /\.vue\.d\.ts$/;
const VUE_TEST_TS_REGEXP = /\.vue\.__verter_test\.ts$/;
const RELATIVE_REGEXP = /^\.\.?($|[\\/])/;

export type VuePublicApiMode = "public" | "testing";

const isRelative = (fileName: string) => RELATIVE_REGEXP.test(fileName);

export function normalizePath(fileName: string): string {
  return fileName.replace(/\\/g, "/");
}

export function getVueVirtualFileInfo(
  fileName: string,
): { sourceFileName: string; mode: VuePublicApiMode } | null {
  const normalized = normalizePath(fileName);

  if (VUE_TEST_TS_REGEXP.test(normalized)) {
    return {
      sourceFileName: normalized.slice(0, -".__verter_test.ts".length),
      mode: "testing",
    };
  }

  if (VUE_D_TS_REGEXP.test(normalized)) {
    return {
      sourceFileName: normalized.slice(0, -".d.ts".length),
      mode: "public",
    };
  }

  if (VUE_TS_REGEXP.test(normalized)) {
    return {
      sourceFileName: normalized.slice(0, -".ts".length),
      mode: "public",
    };
  }

  return null;
}

export function toVueVirtualFileName(
  fileName: string,
  mode: VuePublicApiMode,
): string {
  const normalized = normalizePath(fileName);
  return mode === "testing"
    ? `${normalized}.__verter_test.ts`
    : `${normalized}.ts`;
}

export function stripVueVirtualSuffix(fileName: string): string {
  return getVueVirtualFileInfo(fileName)?.sourceFileName ?? normalizePath(fileName);
}

export function isLikelyTestFileName(fileName: string): boolean {
  const normalized = normalizePath(fileName);
  return (
    /(?:^|\/)__tests__(?:\/|$)/.test(normalized) ||
    /(?:^|\/)__specs__(?:\/|$)/.test(normalized) ||
    /(?:^|\/)[^/]+\.(?:spec|test)\.[^/]+$/i.test(normalized)
  );
}

export function resolveVuePublicApiMode(
  exposeBindingsTesting: boolean,
  containingFile: string,
  isTestFile: (fileName: string) => boolean,
): VuePublicApiMode {
  if (!exposeBindingsTesting) {
    return "public";
  }

  return isTestFile(stripVueVirtualSuffix(containingFile)) ? "testing" : "public";
}

export const isVue = (fileName: string) => DEFAULT_REGEXP.test(fileName);
export const isRelativeVue = (fileName: string) => isVue(fileName) && isRelative(fileName);

export const isVueTs = (fileName: string) => VUE_TS_REGEXP.test(fileName);
export const isRelativeVueTs = (fileName: string) => isVueTs(fileName) && isRelative(fileName);
export const isVueTestingTs = (fileName: string) => VUE_TEST_TS_REGEXP.test(fileName);
