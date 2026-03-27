import { defineConfig, type PlaywrightTestConfig } from "@playwright/test";

const bundler = process.env.E2E_BUNDLER || "vite";

interface BundlerConfig {
  devPort?: number;
  buildPort: number;
  hasDev: boolean;
}

const bundlers: Record<string, BundlerConfig> = {
  vite: { devPort: 3101, buildPort: 4101, hasDev: true },
  webpack: { devPort: 3102, buildPort: 4102, hasDev: true },
  rspack: { devPort: 3103, buildPort: 4103, hasDev: true },
  farm: { devPort: 3104, buildPort: 4104, hasDev: true },
  rollup: { buildPort: 4105, hasDev: false },
  esbuild: { buildPort: 4106, hasDev: false },
  rolldown: { buildPort: 4107, hasDev: false },
};

const config = bundlers[bundler];
if (!config) {
  throw new Error(`Unknown bundler: ${bundler}. Valid: ${Object.keys(bundlers).join(", ")}`);
}

const projects: PlaywrightTestConfig["projects"] = [];

if (config.hasDev && config.devPort) {
  projects.push({
    name: `${bundler}-dev`,
    use: {
      baseURL: `http://localhost:${config.devPort}`,
    },
  });
}

projects.push({
  name: `${bundler}-build`,
  use: {
    baseURL: `http://localhost:${config.buildPort}`,
  },
});

export default defineConfig({
  testDir: "./tests",
  timeout: 30000,
  retries: 0,
  use: {
    headless: true,
    trace: "on-first-retry",
  },
  projects,
  reporter: [["list"], ["html", { open: "never", outputFolder: `playwright-report/${bundler}` }]],
});
