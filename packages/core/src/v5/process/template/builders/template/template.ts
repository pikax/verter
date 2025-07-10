import { DefaultPlugins } from "../..";
import { TemplateItem } from "../../../../parser";
import { ProcessContext } from "../../../types";
import { processTemplate, TemplateContext } from "../../template";

export function buildTemplate(
  items: TemplateItem[],
  context: Partial<TemplateContext> &
    Pick<
      ProcessContext,
      "filename" | "s" | "blocks" | "block" | "blockNameResolver"
    >
) {
  return processTemplate(items, [...DefaultPlugins], context);
}
