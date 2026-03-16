import { mkdirSync, readdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = dirname(dirname(fileURLToPath(import.meta.url)));
const distDir = join(packageDir, "dist");

mkdirSync(distDir, { recursive: true });

for (const entry of readdirSync(distDir, { withFileTypes: true })) {
  if (!entry.isFile() || !entry.name.endsWith(".node")) {
    continue;
  }
  rmSync(join(distDir, entry.name));
}
