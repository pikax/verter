import { BlockPlugin } from "../../../template/plugins";
import { ProcessItemType } from "../../../types";
import { definePlugin } from "../../types";

export const BindingContextPlugin = definePlugin({
  name: "VerterBindingContext",

  post(s, ctx) {
    // this should update the <script > ... with the function bindings
    // then return it
    // and then have the fullBinding context
    const block = ctx.block.block.block;
    const tag = ctx.block.block.tag;

    const isTs = block.lang?.startsWith("ts");
    const isSetup = !!block.attrs.setup;
    const isAsync = ctx.isAsync;
    const genericInfo = ctx.generic;

    const bindingContext = ctx.prefix("BindingContext");

    // remove close tag
    s.update(tag.pos.close.start, tag.pos.close.end, "");

    // handle attributes

    const attribute = tag.attributes.attributes;
    if (attribute && attribute.value) {
      const ATTRIBUTES = ctx.prefix("ATTRIBUTES");
      const prefix = ctx.prefix("");
      const preAttributes = `\n/**\n * ${ATTRIBUTES}\n */type `;

      s.prependRight(attribute.start, prefix);
      s.prependRight(attribute.start, preAttributes);
      s.prependLeft(
        attribute.key.end,
        `${genericInfo ? `<${genericInfo.source}>` : ""}`
      );

      // remove delimiter
      s.remove(attribute.value.start - 1, attribute.value.start);
      s.overwrite(attribute.value.end, attribute.value.end + 1, ";");

      // // move attribute to the end
      s.move(attribute.start, attribute.end, tag.pos.close.end);
    }

    if (isSetup) {
      const preGeneric = `export ${
        isAsync ? "async " : ""
      }function ${bindingContext}`;
      const postGeneric = `(){`;
      s.update(tag.pos.open.start, tag.pos.content.start, preGeneric);

      const generic = tag.attributes.generic;
      if (genericInfo && generic && generic.value) {
        // remove generic=" "
        s.overwrite(generic.start, generic.value.start, "<");
        s.overwrite(generic.end - 1, generic.end, `>`);

        // append postGeneric
        s.prependLeft(generic.end, postGeneric);
      } else {
        s.prependLeft(tag.pos.content.start, postGeneric);
      }

      // remove unprocessed attributes
      const ignoreAttributes = new Set(["attributes", "generic"]);
      for (const key in tag.attributes) {
        if (ignoreAttributes.has(key)) continue;
        const value = tag.attributes[key];

        s.remove(value.start, value.end);
      }

      // remove > from open tag
      // using update to prevent removal if something was already prepended
      s.update(tag.pos.open.end - 1, tag.pos.open.end, "");

      const templateBindings =
        ctx.blocks.find((x) => x.type === "template")?.result?.items ?? [];

      // handle return
      const returnItemsStr = Array.from(
        ctx.items
          .filter((x) => x.type === ProcessItemType.Binding)
          .reduce((acc, x) => {
            const g = x.group ?? "";
            let items = acc.get(g);
            if (!items) {
              items = [];
              acc.set(g, items);
            }
            items.push(x.name);
            return acc;
          }, new Map<string, string[]>())
          .entries()
      )
        .map(([group, names]) => {
          const mapped = names.map(
            (x) => `${x}${isTs ? `:${x} as typeof ${x}` : ""}`
          );

          if (group) {
            return `${group}:{${mapped.join(",")}}`;
          }
          return mapped.join(",");
        })
        .join(",");

      s.prependLeft(tag.pos.close.end, `;return{${returnItemsStr}};`);

      // close
      s.appendLeft(tag.pos.close.end, "}");
    } else {
      // todo ME
    }
  },

  // // add known bindings
  // transformDeclaration(item, s, ctx) {
  //   if (!item.name) return;
  //   const binding = ctx.prefix("binding");
  //   ctx.items.push({
  //     type: ProcessItemType.Binding,
  //     name: item.name,
  //     group: binding,
  //   });
  // },
});
