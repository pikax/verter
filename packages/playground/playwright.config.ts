import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30000,
  retries: 0,
  use: {
    baseURL: "http://localhost:5173",
    headless: true,
  },
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
      },
    },
  ],
  /* Do NOT start webServer here — we manage it externally */
});
