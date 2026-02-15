export const EXPORT_HELPER_ID = "\0plugin-vue:export-helper";

/**
 * The export helper function matches @vitejs/plugin-vue's _export_sfc.
 * It applies metadata (like __scopeId) to Vue components, handling both
 * script setup (direct assignment) and Options API (__vccOpts fallback).
 */
export const EXPORT_HELPER_CODE = `
export default (sfc, props) => {
  const target = sfc.__vccOpts || sfc;
  for (const [key, val] of props) {
    target[key] = val;
  }
  return target;
}
`;
