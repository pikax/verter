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
  },

  // add known bindings
  transformDeclaration(item, s, ctx) {
    if (
      item.parent.type === "VariableDeclarator" &&
      item.parent.init?.type === "CallExpression"
    ) {
      if (item.parent.init.callee.type === "Identifier") {
        const name = item.parent.init.callee.name;
        this.items.add(name);
      }
    }
  },
  transformFunctionCall(item, s, ctx) {
    this.items.add(item.name);
  },
});
