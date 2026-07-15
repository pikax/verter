type MountScriptBuilder = (moduleKey: string) => string;

/**
 * Browser preview adapters are registered beside their mount implementation so
 * capability checks cannot drift from executable behavior. Object keys avoid a
 * second hand-maintained framework array; the generated framework manifest still
 * owns which framework ids exist globally.
 */
const PREVIEW_RUNTIME_MOUNT_BUILDERS = {
  vue: (moduleKey) => `
    const Component = window.__modules__[${moduleKey}]?.default
    if (Component) {
      const target = document.getElementById('app')
      const app = window.Vue.createApp(Component)
      window.__currentApp__ = app
      app.mount(target)
    }
  `,
  svelte: (moduleKey) => `
    const Component = window.__modules__[${moduleKey}]?.default
    if (Component) {
      const target = document.getElementById('app')
      const instance = window.SvelteInternalClient.mount(Component, { target })
      window.__currentApp__ = {
        unmount() { return window.SvelteInternalClient.unmount(instance) }
      }
    }
  `,
} as const satisfies Record<string, MountScriptBuilder>;

export type PreviewRuntimeFramework = keyof typeof PREVIEW_RUNTIME_MOUNT_BUILDERS;

export const PREVIEW_RUNTIME_FRAMEWORK_IDS: readonly PreviewRuntimeFramework[] = Object.freeze(
  Object.keys(PREVIEW_RUNTIME_MOUNT_BUILDERS) as PreviewRuntimeFramework[],
);

/** Build the framework-owned mount adapter evaluated inside the preview iframe. */
export function buildPreviewMountScript(
  frameworkId: PreviewRuntimeFramework,
  mainModule: string,
): string {
  return PREVIEW_RUNTIME_MOUNT_BUILDERS[frameworkId](JSON.stringify(mainModule));
}
