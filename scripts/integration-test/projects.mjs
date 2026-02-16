/**
 * Integration test project definitions.
 *
 * SYNC RULE: This file and `.github/workflows/integration-test.yml` define the
 * same project matrix.  When adding, removing, or modifying a project here you
 * MUST update the CI workflow matrix to match, and vice-versa.
 *
 * Field mapping (JS → YAML):
 *   name           → name
 *   repo           → repo
 *   branch         → branch
 *   buildCmd       → build-cmd
 *   testCmd         → test-cmd
 *   packageManager → package-manager
 *   bundler        → bundler
 */

/** @typedef {'vite' | 'rollup' | 'nuxt'} Bundler */
/** @typedef {'pnpm' | 'npm'} PackageManager */

/**
 * @typedef {Object} Project
 * @property {string}         name           - Short identifier (used in CLI, logs, directory names)
 * @property {string}         repo           - GitHub `owner/repo`
 * @property {string}         branch         - Branch to clone / checkout
 * @property {string}         buildCmd       - Shell command to build the project
 * @property {string}         testCmd        - Shell command to run tests (empty string = no tests)
 * @property {PackageManager} packageManager - Package manager used by the project
 * @property {Bundler}        bundler        - Build tool that loads the Vue plugin
 */

/** @type {Project[]} */
export const projects = [
  // ── Existing CI projects ───────────────────────────────────────────
  {
    name: 'vuetify',
    repo: 'vuetifyjs/vuetify',
    branch: 'master',
    buildCmd: 'pnpm run build',
    testCmd: 'pnpm --filter vuetify test:unit',
    packageManager: 'pnpm',
    bundler: 'vite',
  },
  {
    name: 'oku-primitives',
    repo: 'oku-ui/primitives',
    branch: 'main',
    buildCmd: 'pnpm run build',
    testCmd: 'pnpm run test',
    packageManager: 'pnpm',
    bundler: 'vite',
  },
  {
    name: 'hoppscotch',
    repo: 'hoppscotch/hoppscotch',
    branch: 'main',
    buildCmd: 'pnpm --filter @hoppscotch/selfhost-web build',
    testCmd: 'pnpm --filter @hoppscotch/common test',
    packageManager: 'pnpm',
    bundler: 'vite',
  },
  {
    name: 'element-plus',
    repo: 'element-plus/element-plus',
    branch: 'dev',
    buildCmd: 'pnpm run build',
    testCmd: '',
    packageManager: 'pnpm',
    bundler: 'rollup',
  },

  // ── New projects ───────────────────────────────────────────────────
  {
    name: 'coreui',
    repo: 'coreui/coreui-free-vue-admin-template',
    branch: 'main',
    buildCmd: 'npm run build',
    testCmd: '',
    packageManager: 'npm',
    bundler: 'vite',
  },
  {
    name: 'balancer-frontend-v2',
    repo: 'balancer/frontend-v2',
    branch: 'develop',
    buildCmd: 'npm run build:withouttokenlists',
    testCmd: 'npm run test:unit',
    packageManager: 'npm',
    bundler: 'vite',
  },
  {
    name: 'shadcn-vue',
    repo: 'unovue/shadcn-vue',
    branch: 'dev',
    buildCmd: 'pnpm --filter shadcn-vue build',
    testCmd: 'pnpm --filter shadcn-vue test',
    packageManager: 'pnpm',
    bundler: 'vite',
  },
  {
    name: 'slidev',
    repo: 'slidevjs/slidev',
    branch: 'main',
    buildCmd: 'pnpm -r --filter="./packages/**" --parallel run build',
    testCmd: 'pnpm run test',
    packageManager: 'pnpm',
    bundler: 'vite',
  },
  {
    name: 'zyronon-douyin',
    repo: 'zyronon/douyin',
    branch: 'master',
    buildCmd: 'pnpm run build',
    testCmd: '',
    packageManager: 'pnpm',
    bundler: 'vite',
  },
  {
    name: 'primevue',
    repo: 'primefaces/primevue',
    branch: 'master',
    buildCmd: 'pnpm run build:packages',
    testCmd: '',
    packageManager: 'pnpm',
    bundler: 'rollup',
  },
  {
    name: 'ant-design-vue',
    repo: 'vueComponent/ant-design-vue',
    branch: 'main',
    buildCmd: 'npm run build',
    testCmd: 'npm run test',
    packageManager: 'npm',
    bundler: 'vite',
  },

  // ── Nuxt projects ──────────────────────────────────────────────────
  {
    name: 'nuxt-ui',
    repo: 'nuxt/ui',
    branch: 'v4',
    buildCmd: 'pnpm nuxt-module-build prepare && pnpm build',
    testCmd: 'pnpm test',
    packageManager: 'pnpm',
    bundler: 'nuxt',
  },

  // ── Large Vue ecosystem projects ───────────────────────────────────
  {
    name: 'vue-vben-admin',
    repo: 'vbenjs/vue-vben-admin',
    branch: 'main',
    buildCmd: 'pnpm build',
    testCmd: '',
    packageManager: 'pnpm',
    bundler: 'vite',
  },
  {
    name: 'vant',
    repo: 'youzan/vant',
    branch: 'main',
    buildCmd: 'pnpm build',
    testCmd: 'pnpm test',
    packageManager: 'pnpm',
    bundler: 'vite',
  },
  {
    name: 'naive-ui',
    repo: 'tusen-ai/naive-ui',
    branch: 'main',
    buildCmd: 'pnpm build:package',
    testCmd: 'pnpm vitest run',
    packageManager: 'pnpm',
    bundler: 'vite',
  },
  {
    name: 'tdesign-vue-next',
    repo: 'Tencent/tdesign-vue-next',
    branch: 'develop',
    buildCmd: 'pnpm run build',
    testCmd: 'pnpm run test',
    packageManager: 'pnpm',
    bundler: 'vite',
  },
  {
    name: 'radix-vue',
    repo: 'unovue/radix-vue',
    branch: 'v2',
    buildCmd: 'pnpm --filter reka-ui build',
    testCmd: 'pnpm --filter reka-ui test',
    packageManager: 'pnpm',
    bundler: 'vite',
  },
];
