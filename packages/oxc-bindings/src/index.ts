import { resolveBinding } from "./resolveBinding";
import { resolve } from "node:path";

export async function resolveAndDownloadBinding(toPath: string) {
  const binding = resolveBinding();
  console.log("trying to resolve binding", binding);

  let exists = false;

  try {
    exists = !!require.resolve(binding);
  } catch (e) {}
  console.log("binding", binding, "found", exists);

  if (!exists) {
    await import("./download").then((x) =>
      x.downloadPackage(binding, resolve(toPath, "node_modules", binding))
    );

    console.log("Verter: downloaded and extracted", binding);
  }
}
