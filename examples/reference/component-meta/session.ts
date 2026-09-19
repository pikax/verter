import { openComponentMetaSession } from "@verter/component-meta";

export async function inspect(root: string, filePath: string) {
  const session = await openComponentMetaSession({
    root,
    tsconfig: "./tsconfig.json",
  });
  try {
    return await session.getComponentMeta(filePath);
  } finally {
    session.close();
  }
}
