import { BlockPlugin } from "../../../template/plugins";
import { ProcessItemType } from "../../../types";
import { definePlugin } from "../../types";

export const BindingContextPlugin = definePlugin({
  name: "VerterBindingContext",
  items: new Set<string>(),

  pre() {
    this.items.clear();
  },

  post(s, ctx) {
    // this should update the <script > ... with the function bindings
    // then return it
    // and then have the fullBinding context

    const block = ctx.block.block.block;
    const tag = ctx.block.block.tag;

    const isTs = block.lang?.startsWith("ts");
    const isSetup = !!block.attrs.setup;
    const isAsync = ctx.isAsync;
    const generic = ctx.generic;

    const bindingContext = ctx.prefix("BindingContext");

    // remove close tag
    s.update(tag.pos.close.start, tag.pos.close.end, "");

    let attrStart: number | undefined = undefined;
    let attrEnd: number | undefined = undefined;

    let genStart: number | undefined = undefined;
    let genEnd: number | undefined = undefined;

    // handle attributes
    if (typeof block.attrs.attributes === "string") {
      const attributesContentStart =
        tag.content.indexOf(block.attrs.attributes) + tag.pos.content.start;
      const attributesStart = attributesContentStart - 'attributes="'.length;

      attrStart = attributesStart - 1;
      attrEnd = attributesContentStart + block.attrs.attributes.length;

      const ATTRIBUTES = ctx.prefix("ATTRIBUTES");
      const prefix = ctx.prefix("");
      const preAttributes = `/**\n * ${ATTRIBUTES}\n */type `;

      s.prependLeft(attributesStart, prefix);
      s.prependLeft(attributesStart, preAttributes);
      s.prependLeft(attrEnd, `${generic ? generic.source : ""};`);

      // remove delimiter
      s.remove(attributesContentStart - 1, attributesContentStart);
      s.remove(
        attributesContentStart + block.attrs.attributes.length,
        attributesContentStart + block.attrs.attributes.length + 1
      );

      // move attribute to the end
      s.move(attrStart, attrEnd, tag.pos.close.end);
    }

    if (isSetup) {
      const preGeneric = `export ${
        isAsync ? "async " : ""
      }function ${bindingContext}`;
      const postGeneric = `(){`;
      s.update(tag.pos.open.start, tag.pos.content.start, preGeneric);

      if (generic) {
        const genericStart =
          tag.content.indexOf(generic.source) + tag.pos.content.start;
        const genericEnd = genericStart + generic.source.length;

        // remove before generic
        s.remove(tag.pos.content.start, genericStart);
        // // remove after generic
        // s.remove(genericEnd, tag.pos.open.end);
        // remove generic delimiter
        s.remove(genericEnd, genericEnd + 1);

        // update generic
        s.prependLeft(genericStart, "<");
        s.prependLeft(genericEnd, ">" + postGeneric);

        genStart = genericStart;
        genEnd = genericEnd;
      } else {
        s.prependLeft(tag.pos.content.start, postGeneric);
        // update tag start to attribute start
        s.remove(tag.pos.content.start + 1, attrStart ?? tag.pos.open.end);

        if (attrStart) {
          s.remove(attrEnd!, tag.pos.open.end);
        }
      }

      //   // remove attributes
      //   const handledAttributes = new Set(["attributes", "generic"]);
      //   Object.keys(block.attrs).forEach((key) => {
      //     if (!handledAttributes.has(key)) {
      //       const value = block.attrs[key];
      //       const start =
      //         tag.content.indexOf(`${key}${value === true ? "" : "="}`);
      //       const end =
      //         value === true
      //           ? start + key.length
      //           : tag.content.indexOf(value, start) +
      //           tag.pos.content.start + value.length;
      //       s.remove(start + tag.pos.content.start, end +tag.pos.content.start);
      //     }
      //   });
      // remove > from open tag
      s.remove(tag.pos.content.end - 1, tag.pos.open.end);

      // handle return

      const extraBindings = ctx.items.filter(
        (x) => x.type === ProcessItemType.Binding
      );
      const returnItems = Array.from(this.items)
        .concat(extraBindings.map((x) => x.name))
        .map((x) => `${x}: typeof ${x}`)
        .join(",");

      s.prependLeft(tag.pos.close.end, `;return {${returnItems}}`);

      // close
      s.appendLeft(tag.pos.close.end, "}");
    } else {
    }
  },

  // add known bindings
  transformDeclaration(item, s, ctx) {
    if (item.name) {
      this.items.add(item.name);
    }
    // if (item.parent.type === "VariableDeclarator") {
    //   if (item.parent.init?.type === "CallExpression") {
    //     if (item.parent.init.callee.type === "Identifier") {
    //       const name = item.parent.init.callee.name;
    //       this.items.add(name);
    //     }
    //   } else if (item.parent.id?.type === "Identifier") {
    //     this.items.add(item.parent.id.name);
    //   }
    // }
  },
  //   transformFunctionCall(item, s, ctx) {
  //     this.items.add(item.name);
  //   },
});
