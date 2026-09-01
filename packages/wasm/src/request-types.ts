// =============================================================================
// Request DTOs owned by this binding
//
// The three request shapes below are declared here rather than imported
// from `@verter/native`, so `@verter/wasm` owns the compatibility contract
// for everything a browser caller passes IN. Their public names and wire
// shapes are unchanged; only the declaration site moved. The leaf types
// they still borrow from native carry no compile-profile reference of
// their own, so no exported declaration of this package reaches the native
// profile.
//
// They live in a leaf module rather than in `index.ts` so `tsc` can check
// them: `index.ts` imports the gitignored wasm-bindgen artifact and cannot
// be type-checked from a clean tree. Declaring a shape a second time is
// only safe if something holds the two copies together, so
// `src/index.test-d.ts` asserts each one is exactly its `@verter/native`
// counterpart. Edit either side and that check fails.
// =============================================================================

import type {
  CompileCacheMode,
  HostBlockOverrideEntry,
  HostVirtualNodeKind,
} from "@verter/native/host-types";

export interface HostCompileProfile {
  filename?: string;
  isProduction?: boolean;
  /** Vue custom-element script policy; unrelated to template `customElements`. */
  customElement?: boolean;
  ssr?: boolean;
  /**
   * SSR asset-collection module id registered on `ssrContext.modules`.
   * Vite's ssr-manifest keys are ROOT-RELATIVE — pass
   * `normalizePath(relative(root, filename))`; absent falls back to the
   * canonical id.
   */
  ssrModuleId?: string;
  hmrStrategy?: "none" | "vite" | "webpack";
  componentId?: string;
  delimiters?: [string, string];
  customElements?: string[];
  comments?: boolean;
  runtimeModuleName?: string;
  typesModuleName?: string;
  forceVapor?: boolean;
  forceJs?: boolean;
  sourceMap?: boolean;
  /** Compilation target preset: "bundler" (default), "ide", or "analysis". */
  target?: "bundler" | "ide" | "analysis";
  /**
   * Inline the render function inside `setup()` (Vue production topology,
   * official `compileScript({ inlineTemplate: true })`). Absent resolves to
   * `isProduction` (official default: inline in prod builds). VDOM client
   * only; Vapor inline and inline SSR fall back to non-inline.
   */
  inline?: boolean;
  /** Requested compile cache mode. Defaults to "session". */
  requestedMode?: CompileCacheMode;
}

export interface HostBlockOverrideRequest {
  canonicalId: string;
  compileProfile?: HostCompileProfile;
  overrides: HostBlockOverrideEntry[];
}

export interface HostVirtualQuery {
  rawId?: string;
  canonicalId?: string;
  nodeKind?: HostVirtualNodeKind;
  compileProfile?: HostCompileProfile;
}
