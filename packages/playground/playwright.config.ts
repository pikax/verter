import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30000,
  retries: 0,
  use: {
    baseURL: "http://localhost:5173",
    headless: true,
  },
  webServer: [
    {
      command: "npx vite --port 5173",
      port: 5173,
      reuseExistingServer: true,
      timeout: 60000,
    },
    {
      command: "npx vite build && npx vite preview --port 4173",
      port: 4173,
      reuseExistingServer: true,
      timeout: 120000,
    },
  ],
  projects: [
    {
      name: "dev",
      use: {
        baseURL: "http://localhost:5173",
      },
    },
    {
      name: "preview",
      use: {
        baseURL: "http://localhost:4173",
        navigationTimeout: 15000,
      },
    },
  ],
});
