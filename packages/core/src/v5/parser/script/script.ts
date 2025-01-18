import { VerterAST } from "../ast";
import { shallowWalk } from "../walk";
import { ParsedScriptResult, ScriptItem } from "./types";

import { handleShared } from "./shared/index.js";
import { handleOptionsNode } from "./options/index.js";
import { handleSetupNode } from "./setup/index.js";

export function parseScript(
  ast: VerterAST,
  attrs: Record<string, string | true>
): ParsedScriptResult {
  let isAsync = false;
  const items: ScriptItem[] = [];

  const isSetup = !!attrs.setup;

  const handler = isSetup ? handleSetupNode : handleOptionsNode;

  shallowWalk(ast, (node) => {
    const shared = handleShared(node);
    if (shared) {
      items.push(...shared);
    }
    const result = handler(node);
    if (Array.isArray(result)) {
      items.push(...result);
    } else {
      isAsync = result.isAsync;
      items.push(...result.items);
    }
  });

  return {
    isAsync,
    items,
  };
}
