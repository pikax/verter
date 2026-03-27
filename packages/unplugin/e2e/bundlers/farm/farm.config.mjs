import path from "path";
import { fileURLToPath } from "url";
import vue from "@verter/unplugin/farm";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const appDir = path.resolve(__dirname, "../../app");

export default {
  root: appDir,
  plugins: [vue()],
  server: {
    port: 3104,
    strictPort: true,
  },
  compilation: {
    output: {
      path: path.resolve(__dirname, "dist"),
    },
  },
};
