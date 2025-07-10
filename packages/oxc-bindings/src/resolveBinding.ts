// based on https://github.com/oxc-project/oxc/blob/d5c94a863f24cb9ef28dbccae89079189c54ed4d/napi/parser/bindings.js
import { readFileSync } from "fs";
import { execSync } from "child_process";

// Detect musl on Linux
const isMusl = (): boolean => {
  if (process.platform !== "linux") return false;
  try {
    return readFileSync("/usr/bin/ldd", "utf8").includes("musl");
  } catch {
    try {
      return execSync("ldd --version", { encoding: "utf8" }).includes("musl");
    } catch {
      return false;
    }
  }
};

export function resolveBinding(): string {
  const { platform, arch } = process;

  if (process.env.NAPI_RS_NATIVE_LIBRARY_PATH) {
    return process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
  }

  switch (platform) {
    case "android":
      if (arch === "arm64") return "@oxc-parser/binding-android-arm64";
      if (arch === "arm") return "@oxc-parser/binding-android-arm-eabi";
      break;

    case "win32":
      if (arch === "x64") return "@oxc-parser/binding-win32-x64-msvc";
      if (arch === "ia32") return "@oxc-parser/binding-win32-ia32-msvc";
      if (arch === "arm64") return "@oxc-parser/binding-win32-arm64-msvc";
      break;

    case "darwin":
      if (arch === "x64") return "@oxc-parser/binding-darwin-x64";
      if (arch === "arm64") return "@oxc-parser/binding-darwin-arm64";
      return "@oxc-parser/binding-darwin-universal";

    case "freebsd":
      if (arch === "x64") return "@oxc-parser/binding-freebsd-x64";
      if (arch === "arm64") return "@oxc-parser/binding-freebsd-arm64";
      break;

    case "linux": {
      const musl = isMusl();
      if (arch === "x64") {
        return musl
          ? "@oxc-parser/binding-linux-x64-musl"
          : "@oxc-parser/binding-linux-x64-gnu";
      }
      if (arch === "arm64") {
        return musl
          ? "@oxc-parser/binding-linux-arm64-musl"
          : "@oxc-parser/binding-linux-arm64-gnu";
      }
      if (arch === "arm") {
        return musl
          ? "@oxc-parser/binding-linux-arm-musleabihf"
          : "@oxc-parser/binding-linux-arm-gnueabihf";
      }
      if (arch === "riscv64") {
        return musl
          ? "@oxc-parser/binding-linux-riscv64-musl"
          : "@oxc-parser/binding-linux-riscv64-gnu";
      }
      if (arch === "ppc64") return "@oxc-parser/binding-linux-ppc64-gnu";
      if (arch === "s390x") return "@oxc-parser/binding-linux-s390x-gnu";
      break;
    }
  }

  // WASI fallback
  if (process.env.NAPI_RS_FORCE_WASI) {
    return "@oxc-parser/binding-wasm32-wasi";
  }

  // WebContainer fallback
  if (globalThis.process?.versions?.webcontainer) {
    return "@oxc-parser/binding-wasm32-wasi";
  }

  throw new Error(`Unsupported platform/architecture: ${platform}/${arch}`);
}
