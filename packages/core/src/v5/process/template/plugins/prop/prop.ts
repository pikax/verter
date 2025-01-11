import { MagicString } from "@vue/compiler-sfc";
import { declareTemplatePlugin, TemplatePlugin } from "../../template";
import {
  CAMELIZE,
  DirectiveNode,
  ExpressionNode,
  NodeTypes,
  SourceLocation,
} from "@vue/compiler-core";
import { camelize } from "vue";

function overrideCamelCase(
  loc: SourceLocation,
  s: MagicString,
  ctx: {
    camelWhitelistAttributes(name: string): boolean;
  }
) {
  if (ctx.camelWhitelistAttributes(loc.source)) {
    return;
  }
  const offset = loc.start.offset;
  for (const match of loc.source.matchAll(/-([a-z])/g)) {
    s.overwrite(
      offset + match.index,
      offset + match.index + 2,
      match[1].toUpperCase()
    );
  }
}

export const PropPlugin = declareTemplatePlugin({
  name: "VerterProp",
  used: {
    normalizeStyle: false,
    normalizeClass: false,
  },
  pre() {
    this.used.normalizeStyle = false;
    this.used.normalizeClass = false;
  },

  transformProp(prop, s, ctx) {
    // mergers aka style & class
    if (prop.node === null) {
      const accessorType =
        prop.name === "style" ? "normalizeStyle" : "normalizeClass";
      const normaliseAccessor = ctx.retrieveAccessor(accessorType);

      const nodes = prop.props.map((x) => x.node).filter((x) => x !== null);

      // move to the first directive we find
      const firstDirective = nodes.find(
        (x) => x.type === NodeTypes.DIRECTIVE && x.exp
      ) as DirectiveNode & { exp: ExpressionNode };

      if (!firstDirective) {
        return;
      }

      if (this.used[accessorType] === false) {
        this.used[accessorType] = true;
      }

      // update and handle the directive binding
      if (firstDirective.rawName?.startsWith("v-bind:")) {
        s.remove(
          firstDirective.loc.start.offset,
          firstDirective.loc.start.offset + 7
        );
      } else if (firstDirective.rawName?.startsWith(":")) {
        s.remove(
          firstDirective.loc.start.offset,
          firstDirective.loc.start.offset + 1
        );
      }

      // replace " with { }
      s.overwrite(
        firstDirective.exp.loc.start.offset - 1,
        firstDirective.exp.loc.start.offset,
        "{"
      );
      s.overwrite(
        firstDirective.exp.loc.end.offset,
        firstDirective.exp.loc.end.offset + 1,
        "}"
      );

      s.prependRight(
        firstDirective.exp.loc.start.offset,
        `${normaliseAccessor}([`
      );

      for (let i = 0; i < nodes.length; i++) {
        const node = nodes[i];
        if (node === firstDirective) {
          continue;
        }

        const loc =
          node.type === NodeTypes.DIRECTIVE
            ? (node.exp ?? node.arg)?.loc
            : node.value?.loc;

        if (loc) {
          s.prependRight(loc.start.offset, `,`);
          s.move(
            loc.start.offset,
            loc.end.offset,
            firstDirective.exp.loc.end.offset
          );

          //   s.remove(prop.loc.start.offset, loc.start.offset);
          //   s.remove(loc.end.offset, prop.loc.end.offset);
        }

        const nameLoc =
          node.type === NodeTypes.DIRECTIVE ? node.arg?.loc : node.nameLoc;

        if (nameLoc) {
          s.remove(nameLoc.start.offset, nameLoc.end.offset + 1);
        }
      }

      s.appendRight(firstDirective.exp.loc.end.offset, "])");
    } else if (prop.static) {
      // handle prop
      if (prop.node.value) {
        // append { and } to value
        s.prependRight(prop.node.value.loc.start.offset, "{");
        s.prependLeft(prop.node.value.loc.end.offset, "}");
      }

      if (prop.node.nameLoc) {
        overrideCamelCase(prop.node.nameLoc, s, ctx);
      }
    } else {
      // directive
      const [nameBinding] = prop.arg ?? [];
      // handle camelCase
      if (nameBinding?.ignore === true) {
        // const node = prop.name[0].node as DirectiveNode;
        overrideCamelCase(nameBinding.node.loc, s, ctx);
      }

      // remove v-bind: or :
      const node = prop.node;
      if (node.rawName?.startsWith("v-bind:")) {
        s.remove(node.loc.start.offset, node.loc.start.offset + 7);
      } else if (node.rawName?.startsWith(":")) {
        s.remove(node.loc.start.offset, node.loc.start.offset + 1);
      } else if (node.rawName?.startsWith("v-on:")) {
        if (nameBinding?.ignore === true) {
          s.overwrite(
            node.loc.start.offset + 5,
            node.loc.start.offset + 6,
            nameBinding.name.at(0)?.toUpperCase() ?? ""
          );
        }
        s.overwrite(node.loc.start.offset, node.loc.start.offset + 5, "on");
      } else if (node.rawName?.startsWith("@")) {
        if (nameBinding?.ignore === true) {
          s.overwrite(
            node.loc.start.offset + 1,
            node.loc.start.offset + 2,
            nameBinding.name.at(0)?.toUpperCase() ?? ""
          );
        }
        s.overwrite(node.loc.start.offset, node.loc.start.offset + 1, "on");
      }

      // handle dynamic bindings
      if (node.arg?.loc.source.startsWith("[")) {
        // add {...{
        s.prependRight(node.arg.loc.start.offset, "{...{");
        if (node.exp) {
          // replace ={ to :
          s.overwrite(
            node.exp.loc.start.offset - 2,
            node.exp.loc.start.offset,
            ":"
          );

          // remove last "
          s.remove(node.exp.loc.end.offset, node.exp.loc.end.offset + 1);
        }
        s.prependLeft(node.loc.end.offset, "}}");
      } else {
        // append { and } to value
        if (node.exp) {
          s.overwrite(
            node.exp.loc.start.offset - 1,
            node.exp.loc.start.offset,
            "{"
          );
          s.overwrite(
            node.exp.loc.end.offset,
            node.exp.loc.end.offset + 1,
            "}"
          );
        } else if (nameBinding?.ignore === true) {
          s.appendLeft(node.loc.end.offset, `={${camelize(nameBinding.name)}}`);
        }
      }
    }
  },
});
