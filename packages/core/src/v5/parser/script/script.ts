import { VerterAST } from "../ast";
import { deepWalk, shallowWalk } from "../walk";
import { ParsedScriptResult, ScriptItem } from "./types";

import { createSharedContext, handleShared } from "./shared/index.js";
import { createOptionsContext, handleOptionsNode } from "./options/index.js";
import { createSetupContext, handleSetupNode } from "./setup/index.js";

export function parseScript(
  ast: VerterAST,
  attrs: Record<string, string | true>
): ParsedScriptResult {
  let isAsync = false;
  const items: ScriptItem[] = [];
  const isSetup = !!attrs.setup;
  const lang = attrs.lang || "js";

  if (isSetup) {
    return handleSetupNode(ast, handleShared);
  } else {
    shallowWalk(ast, (node) => {
      const shared = handleShared(node);
      if (shared) {
        items.push(...shared);
      }
      const result = handleOptionsNode(node);
      if (Array.isArray(result)) {
        items.push(...result);
      } else {
        isAsync = result.isAsync;
        items.push(...result.items);
      }
    });
  }

  return {
    isAsync,
    items,
  };
}

export function parseScriptBetter(
  ast: VerterAST,
  attrs: Record<string, string | true>
) {
  // todo default lang to config
  const lang = (typeof attrs.lang === "string" ? attrs.lang : "js") || "js";
  const isSetup = !!attrs.setup;
  const setupCtx = createSetupContext({ lang });
  const sharedCtx = createSharedContext({ lang });
  const optionsCtx = createOptionsContext({ lang, setupCtx });

  const visitorCtx = isSetup ? setupCtx : optionsCtx;

  const items = [] as ScriptItem[];

  function addResult(result: void | ScriptItem | ScriptItem[]) {
    if (!result) return;
    if (Array.isArray(result)) {
      items.push(...result);
    } else {
      items.push(result);
    }
  }

  deepWalk(
    ast,
    function (node, parent, key) {
      const shared = sharedCtx.visit(node, parent, key, this);
      const visit = visitorCtx.visit(node, parent, key, this);
      addResult(shared);
      addResult(visit);
    },
    (node, parent, key) => {
      visitorCtx.leave(node, parent, key);
      sharedCtx.leave(node, parent, key);
    }
  );

  return {
    isAsync: setupCtx.isAsync,
    items,
  };
}
