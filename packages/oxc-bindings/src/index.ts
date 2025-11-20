import { resolveBinding } from "./resolveBinding";
import { resolve } from "node:path";

export async function resolveAndDownloadBinding(toPath: string) {
  const binding = resolveBinding();
  console.log("trying to resolve binding", binding);

  let version = "";

  try {
    const pkg = require(`${binding}/package.json`);
    version = pkg?.version;
  } catch (e) {}

  await import("./download").then((x) => {
    x.downloadPackage(
      binding,
      resolve(toPath, "node_modules", binding),
      version
    );
  });

  console.log("Verter: downloaded and extracted", binding);
}

resolveAndDownloadBinding("./");
