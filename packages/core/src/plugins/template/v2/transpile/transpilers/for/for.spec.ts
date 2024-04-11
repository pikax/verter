import { fromTranspiler } from "../spec.helpers";
import For from "./";

describe("v-for", () => {
  function transpile(
    source: string,
    options?: {
      webComponents: string[];
    }
  ) {
    return fromTranspiler(For, source, [], options);
  }

  it("simple", () => {
    const { result } = transpile(`<li v-for="item in items"></li>`);

    expect(result).toMatchInlineSnapshot(
      `"{__VERTER__renderList(___VERTER__ctx.items,(item)=>{<li ></li>})}"`
    );
  });

  it("destructing", () => {
    const { result } = transpile(`<li v-for="{ message } in items"></li>`);

    expect(result).toMatchInlineSnapshot(
      `"{__VERTER__renderList(___VERTER__ctx.items,({ message })=>{<li ></li>})}"`
    );
  });

  it("index", () => {
    const { result } = transpile(`<li v-for="(item, index) in items"></li>`);

    expect(result).toMatchInlineSnapshot(
      `"{__VERTER__renderList(___VERTER__ctx.items,(item, index)=>{<li ></li>})}"`
    );
  });

  it("index + key", () => {
    const { result } = transpile(
      `<li v-for="(item, index) in items" :key="index + 'random'"></li>`
    );

    expect(result).toMatchInlineSnapshot(
      `"{__VERTER__renderList(___VERTER__ctx.items,(item, index)=>{<li  key={index + 'random'}></li>})}"`
    );
  });

  it("destructing + index", () => {
    const { result } = transpile(
      `<li v-for="({ message }, index) in items"></li>`
    );

    expect(result).toMatchInlineSnapshot(
      `"{__VERTER__renderList(___VERTER__ctx.items,({ message }, index)=>{<li ></li>})}"`
    );
  });

  it("nested", () => {
    const { result } = transpile(`<li v-for="item in items">
  <span v-for="childItem in item.children"></span>
</li>`);

    expect(result).toMatchInlineSnapshot(`
        "{__VERTER__renderList(___VERTER__ctx.items,(item)=>{<li >
              {__VERTER__renderList(item.children,(childItem)=>{<span ></span>})}
          </li>})}"
      `);
  });

  it("of", () => {
    const { result } = transpile(`<li v-for="item of items"></li>`);

    expect(result).toMatchInlineSnapshot(
      `"{__VERTER__renderList(___VERTER__ctx.items,(item)=>{<li ></li>})}"`
    );
  });

  it("of with tab", () => {
    const { result } = transpile(`<li v-for="item   of items"></li>`);

    expect(result).toMatchInlineSnapshot(
      `"{__VERTER__renderList(___VERTER__ctx.items,(item  )=>{<li ></li>})}"`
    );
  });

  it("of with tabs", () => {
    const { result } = transpile(`<li v-for="item   of     items"></li>`);

    expect(result).toMatchInlineSnapshot(
      `"{__VERTER__renderList(    ___VERTER__ctx.items,(item  )=>{<li ></li>})}"`
    );
  });

  // object
  it("object", () => {
    const { result } = transpile(`<li v-for="value in myObject"></li>`);

    expect(result).toMatchInlineSnapshot(
      `"{__VERTER__renderList(___VERTER__ctx.myObject,(value)=>{<li ></li>})}"`
    );
  });

  it("object + key", () => {
    const { result } = transpile(`<li v-for="(value, key) in myObject"></li>`);

    expect(result).toMatchInlineSnapshot(
      `"{__VERTER__renderList(___VERTER__ctx.myObject,(value, key)=>{<li ></li>})}"`
    );
  });

  it("object + key + index", () => {
    const { result } = transpile(
      `<li v-for="(value, key, index) in myObject"></li>`
    );

    expect(result).toMatchInlineSnapshot(
      `"{__VERTER__renderList(___VERTER__ctx.myObject,(value, key, index)=>{<li ></li>})}"`
    );
  });

  // range
  it("range", () => {
    const { result } = transpile(`<li v-for="n in 10"></li>`);

    expect(result).toMatchInlineSnapshot(
      `"{__VERTER__renderList(10,(n)=>{<li ></li>})}"`
    );
  });

  // v-if has higher priority than v-for
  it("v-if", () => {
    const { result } = transpile(
      `<li v-for="i in items" v-if="items > 5"></li>`
    );

    expect(result).toMatchInlineSnapshot(
      `"{(___VERTER__ctx.items > 5)?__VERTER__renderList(___VERTER__ctx.items,(i)=>{!((___VERTER__ctx.items > 5)) ? undefined : <li  ></li>}) : undefined}"`
    );
  });

  it("should not append ctx to item.", () => {
    const { result } = transpile(`<li v-for="item in items">
      {{ item. }}            
      </li>`);

    expect(result).toMatchInlineSnapshot(`
        "{__VERTER__renderList(___VERTER__ctx.items,(item)=>{<li >
                  { item. }            
                  </li>})}"
      `);
  });

  it("should append ctx to item", () => {
    const { result } = transpile(`<li v-for="item in items">
      {{ foo. }}            
      </li>`);

    expect(result).toMatchInlineSnapshot(`
        "{__VERTER__renderList(___VERTER__ctx.items,(item)=>{<li >
                  { ___VERTER__ctx.foo. }            
                  </li>})}"
      `);
  });
});
