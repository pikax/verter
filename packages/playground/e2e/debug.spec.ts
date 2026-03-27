import { test, expect } from "@playwright/test";

test("debug page state - immediate", async ({ page }) => {
  test.setTimeout(30000);

  const messages: string[] = [];
  page.on("console", (msg) => {
    messages.push(`[${msg.type()}] ${msg.text()}`);
  });
  page.on("pageerror", (err) => {
    messages.push(`[pageerror] ${err.message}`);
  });

  await page.goto("/", { timeout: 15000, waitUntil: "domcontentloaded" });

  // Check immediately (before WASM init) if basic JS works
  const immediate = await page.evaluate(() => document.title).catch(() => "FAILED");
  console.log("Title immediately:", immediate);

  // Wait 2 seconds, check again
  await page.waitForTimeout(2000);
  const after2s = await page
    .evaluate(() => {
      return document.getElementById("app")?.childElementCount ?? -1;
    })
    .catch(() => -99);
  console.log("App children after 2s:", after2s);

  // Wait 5 more seconds
  await page.waitForTimeout(5000);
  const after7s = await page
    .evaluate(() => {
      const app = document.getElementById("app");
      return {
        children: app?.childElementCount ?? -1,
        text: app?.textContent?.substring(0, 200) ?? "NULL",
        html: app?.innerHTML?.substring(0, 500) ?? "NULL",
      };
    })
    .catch(() => ({ children: -99, text: "TIMEOUT", html: "TIMEOUT" }));
  console.log("App children after 7s:", after7s.children);
  console.log("App text after 7s:", JSON.stringify(after7s.text));
  console.log("App HTML after 7s:", after7s.html);

  console.log("=== MESSAGES (first 10) ===");
  messages.slice(0, 10).forEach((m) => console.log(m));
  console.log(`Total messages: ${messages.length}`);

  expect(true).toBe(true);
});
