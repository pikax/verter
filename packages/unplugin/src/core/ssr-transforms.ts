/**
 * SSR/client dead-code elimination transforms.
 *
 * Replaces `import.meta.server`, `import.meta.client`, and `import.meta.env.SSR`
 * with boolean literals so bundlers can tree-shake dead branches.
 */

/**
 * Replace SSR-related `import.meta` expressions with boolean literals.
 *
 * - SSR build: `import.meta.server` → `true`, `import.meta.client` → `false`, `import.meta.env.SSR` → `true`
 * - Client build: `import.meta.server` → `false`, `import.meta.client` → `true`, `import.meta.env.SSR` → `false`
 */
export function replaceImportMetaSsr(code: string, isSSR: boolean): string {
  // Only process if the code contains import.meta references
  if (!code.includes("import.meta.")) return code;

  let result = code;

  // Replace import.meta.env.SSR first (longer match, avoids partial replacement)
  result = result.replaceAll("import.meta.env.SSR", isSSR ? "true" : "false");

  // Replace import.meta.server and import.meta.client
  result = result.replaceAll("import.meta.server", isSSR ? "true" : "false");
  result = result.replaceAll("import.meta.client", isSSR ? "false" : "true");

  return result;
}

/**
 * Strip component tags from compiled output by replacing their render calls
 * with comment placeholders. Works on the compiled JS output (not SFC source).
 *
 * Replaces `_resolveComponent("ComponentName")` calls with a no-op that renders
 * an empty comment node.
 */
export function stripComponents(code: string, componentNames: string[]): string {
  if (componentNames.length === 0) return code;

  let result = code;
  for (const name of componentNames) {
    // Replace _resolveComponent("Name") with a function returning comment VNode
    const pattern = `_resolveComponent("${name}")`;
    if (result.includes(pattern)) {
      result = result.replaceAll(pattern, `(() => ({ __name: "${name}", render: () => null }))()`);
    }
  }
  return result;
}
