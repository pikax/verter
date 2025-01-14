import { VerterAST } from "../../../../parser/ast";
import { ScriptItem } from "../../../../parser/script/types";
import { ProcessContext } from "../../../types";
import { processScript, ScriptContext } from "../../script";

// import 

export function buildBundle(
  items: ScriptItem[],
  _context: Partial<ScriptContext> &
    Pick<ProcessContext, "filename" | "s" | "blocks">) {

    
    return processScript(items, [], _context);
}