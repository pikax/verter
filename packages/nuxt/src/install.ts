import { VERTER_VITE_PLUGIN } from "@verter/unplugin/vite";

export interface InstallOptions {
  /** Also remove `vite:vue-jsx`. */
  readonly replaceJsx: boolean;
}

function isObject(value: unknown): value is Record<PropertyKey, unknown> {
  return typeof value === "object" && value !== null;
}

function containsVerterPlugin(plugins: unknown[]): boolean {
  return plugins.some((entry) =>
    Array.isArray(entry)
      ? containsVerterPlugin(entry)
      : isObject(entry) && entry[VERTER_VITE_PLUGIN] === true,
  );
}

function removeStockPlugins(plugins: unknown[], names: ReadonlySet<string>): void {
  for (let i = plugins.length - 1; i >= 0; i--) {
    const entry = plugins[i];
    if (Array.isArray(entry)) {
      removeStockPlugins(entry, names);
    } else if (isObject(entry) && typeof entry.name === "string" && names.has(entry.name)) {
      plugins.splice(i, 1);
    }
  }
}

/**
 * Replace the stock Vue plugin with Verter's in `plugins`, mutating the array
 * (and nested arrays) in place. Does nothing when Verter's plugin is already
 * present. Returns `true` when it installed.
 */
export function installVerterPlugin(
  plugins: unknown[],
  create: () => object,
  options: InstallOptions,
): boolean {
  if (containsVerterPlugin(plugins)) return false;

  const names = new Set(["vite:vue"]);
  if (options.replaceJsx) names.add("vite:vue-jsx");
  removeStockPlugins(plugins, names);

  plugins.push(create());
  return true;
}
