import { fromTranspiler } from "../spec.helpers";
import Element from "./";

describe("tranpiler element", () => {
  function transpile(
    source: string,
    options?: {
      webComponents: string[];
    }
  ) {
    return fromTranspiler(Element, source, [], options);
  }

  describe("element", () => {
    it("simple", () => {
      const { result } = transpile(`<div></div>`);
      expect(result).toMatchInlineSnapshot(`"<div></div>"`);
    });

    it("self-closing", () => {
      const { result } = transpile(`<div/>`);
      expect(result).toMatchInlineSnapshot(`"<div/>"`);
    });

    it("with children", () => {
      const { result } = transpile(`<div><span>{{text}}</span></div>`);
      expect(result).toMatchInlineSnapshot(
        `"<div><span>{{text}}</span></div>"`
      );
    });
  });

  describe("component", () => {
    it("simple", () => {
      const { result } = transpile(`<my-component></my-component>`);
      expect(result).toMatchInlineSnapshot(
        `"<___VERTER___comp.MyComponent></___VERTER___comp.MyComponent>"`
      );
    });
    it("self-closing", () => {
      const { result } = transpile(`<my-component/>`);
      expect(result).toMatchInlineSnapshot(`"<___VERTER___comp.MyComponent/>"`);
    });

    it("simple camel", () => {
      const { result } = transpile(`<MyComponent></MyComponent>`);
      expect(result).toMatchInlineSnapshot(
        `"<___VERTER___comp.MyComponent></___VERTER___comp.MyComponent>"`
      );
    });
    it("self-closing camel", () => {
      const { result } = transpile(`<MyComponent/>`);
      expect(result).toMatchInlineSnapshot(`"<___VERTER___comp.MyComponent/>"`);
    });

    describe("children", () => {
      it("with children", () => {
        const { result } = transpile(
          `<my-component><span>{{text}}</span></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          <span>{{text}}</span>
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });

      it("with component children", () => {
        const { result } = transpile(
          `<my-component><my-span>{{text}}</my-span></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          <___VERTER___comp.MySpan v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          {{text}}
          })}

          }}></___VERTER___comp.MySpan>
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });

      it("with #default", () => {
        const { result } = transpile(
          `<my-component><template #default/></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          <___VERTER___template />
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });

      it("with v-slot", () => {
        const { result } = transpile(
          `<my-component><template v-slot/></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{
          <___VERTER___template />
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });
      it("with v-slot name", () => {
        const { result } = transpile(
          `<my-component><template v-slot:name/></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.name)(()=>{
          <___VERTER___template />
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });

      it("dynamic # name", () => {
        const { result } = transpile(
          `<my-component><template #[bar]/></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots[___VERTER___ctx.bar])(()=>{

          <___VERTER___template />
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });

      it("dynamic v-slot name", () => {
        const { result } = transpile(
          `<my-component><template v-slot:[bar]/></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots[___VERTER___ctx.bar])(()=>{
          <___VERTER___template />
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });

      it("with #default=props", () => {
        const { result } = transpile(
          `<my-component><template #default="props"/></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)((props)=>{
          <___VERTER___template />
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });

      it("with v-slot=props", () => {
        const { result } = transpile(
          `<my-component><template v-slot="props"/></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)((props)=>{
          <___VERTER___template />
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });
      it("with v-slot name=props", () => {
        const { result } = transpile(
          `<my-component><template v-slot:name="props"/></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.name)((props)=>{
          <___VERTER___template />
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });
      it("with v-slot snake name", () => {
        const { result } = transpile(
          `<my-component><template v-slot:name-foo="props"/></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots['name-foo'])((props)=>{
          <___VERTER___template />
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });

      it("dynamic # name=props", () => {
        const { result } = transpile(
          `<my-component><template #[bar]="props"/></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots[___VERTER___ctx.bar])((props)=>{
          <___VERTER___template />
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });

      it("dynamic v-slot name=props", () => {
        const { result } = transpile(
          `<my-component><template v-slot:[bar]="props"/></my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots[___VERTER___ctx.bar])((props)=>{
          <___VERTER___template />
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });

      it("with slot comment", () => {
        const { result } = transpile(
          `<my-component>
          <!-- NOTE COMMENT HANDLED BY Comment Transpiler -->
          <template #default/>
        </my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          <!-- NOTE COMMENT HANDLED BY Comment Transpiler -->
                    <___VERTER___template />
          })}

          }}>
                    
                  </___VERTER___comp.MyComponent>"
        `);
      });

      it("multiple slots", () => {
        const { result } = transpile(
          `<my-component>
            <template #header/>
            <template #default/>
          </my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.header)(()=>{

          <___VERTER___template />
          })}

          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          <___VERTER___template />
          })}

          }}>
                      
                      
                    </___VERTER___comp.MyComponent>"
        `);
      });
      it("with slot and implicit default", () => {
        const { result } = transpile(
          `<my-component>
          <template #default/>
          <span>{{test}}</span>
        </my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{


          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          <___VERTER___template />
          })}
          <span>{{test}}</span>
          })}

          }}>
                    
                    
                  </___VERTER___comp.MyComponent>"
        `);
      });
      it("implicit default with default", () => {
        const { result } = transpile(
          `<my-component>
          <span>{{test}}</span>
          <template #default/>
        </my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{


          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          <___VERTER___template />
          })}
          <span>{{test}}</span>
          })}

          }}>
                    
                    
                  </___VERTER___comp.MyComponent>"
        `);
      });
      it("implicit default + comment and default", () => {
        const { result } = transpile(
          `<my-component>
          <span>{{test}}</span>
          <!-- NOTE COMMENTS ARE HANDLED BY Comment Transpiler -->
          <template #default/>
        </my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{


          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          <!-- NOTE COMMENTS ARE HANDLED BY Comment Transpiler -->
                    <___VERTER___template />
          })}
          <span>{{test}}</span>
          })}

          }}>
                    
                    
                  </___VERTER___comp.MyComponent>"
        `);
      });

      it("default + condition", () => {
        const { result } = transpile(`<Test>
  <template v-if="true">
    <div />
  </template>
</Test>`);
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.Test v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          { ()=> {if(true){<___VERTER___template >
              <div />
            </___VERTER___template>}}}
          })}

          }}>
            
          </___VERTER___comp.Test>"
        `);
      });

      it("named + condition", () => {
        const { result } = transpile(`<Test>
<template v-if="true" #test>
  <div />
</template>
</Test>`);
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.Test v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.test)(()=>{

          { ()=> {if(true){<___VERTER___template  >
            <div />
          </___VERTER___template>}}}
          })}

          }}>

          </___VERTER___comp.Test>"
        `);
      });

      it("should wrap slot names", () => {
        const { result } = transpile(`<Test>
          <template v-if="true" #test-name>
            <div />
          </template>
        </Test>`);
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.Test v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots['test-name'])(()=>{

          { ()=> {if(true){<___VERTER___template  >
                      <div />
                    </___VERTER___template>}}}
          })}

          }}>
                    
                  </___VERTER___comp.Test>"
        `);
      });

      it("should wrap slot names with props", () => {
        const { result } = transpile(`<Test>
          <template v-if="true" #test-name="{ foo }">
            <div />
          </template>
        </Test>`);
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.Test v-slot={(___VERTER___componentInstance): any=>{
          const $slots = ___VERTER___componentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots['test-name'])(({ foo })=>{
          { ()=> {if(true){<___VERTER___template  >
                      <div />
                    </___VERTER___template>}}}
          })}

          }}>
                    
                  </___VERTER___comp.Test>"
        `);
      });
    });
  });

  describe("webcomponent", () => {
    it("simple", () => {
      const { result } = transpile(`<my-component> </my-component>`, {
        webComponents: ["my-component"],
      });
      expect(result).toMatchInlineSnapshot(`"<my-component> </my-component>"`);
    });
    it("self-closing", () => {
      const { result } = transpile(`<my-component/>`, {
        webComponents: ["my-component"],
      });
      expect(result).toMatchInlineSnapshot(`"<my-component/>"`);
    });

    it("different casing", () => {
      const { result } = transpile(`<my-component> </my-component>`, {
        webComponents: ["MyComponent"],
      });
      expect(result).toMatchInlineSnapshot(`"<my-component> </my-component>"`);
    });
  });

  describe("attributes", () => {
    it("simple", () => {
      const { result } = transpile(`<div test="hello"></div>`);
      expect(result).toMatchInlineSnapshot(`"<div test="hello"></div>"`);
    });

    it("self-closing", () => {
      const { result } = transpile(`<div test="hello"/>`);
      expect(result).toMatchInlineSnapshot(`"<div test="hello"/>"`);
    });

    it("with children", () => {
      const { result } = transpile(
        `<div test="hello"><span test="hello">{{text}}</span></div>`
      );
      expect(result).toMatchInlineSnapshot(
        `"<div test="hello"><span test="hello">{{text}}</span></div>"`
      );
    });

    it("camelcasing", () => {
      const { result } = transpile(`<MyComp test-prop="hello"/>`);
      expect(result).toMatchInlineSnapshot(
        `"<___VERTER___comp.MyComp testProp="hello"/>"`
      );
    });

    it("not camelcasing elemenet", () => {
      const { result } = transpile(`<div test-prop="hello"/>`);
      expect(result).toMatchInlineSnapshot(`"<div test-prop="hello"/>"`);
    });
  });
  describe("directives", () => {
    describe("binding", () => {
      it("props w/:", () => {
        const { result } = transpile(`<span :foo="bar"/>`);

        expect(result).toMatchInlineSnapshot(
          `"<span foo={___VERTER___ctx.bar}/>"`
        );
      });

      it("props w/v-bind:", () => {
        const { result } = transpile(`<span v-bind:foo="bar"/>`);

        expect(result).toMatchInlineSnapshot(
          `"<span foo={___VERTER___ctx.bar}/>"`
        );
      });

      it("v-bind", () => {
        const { result } = transpile(`<span v-bind="bar"/>`);

        expect(result).toMatchInlineSnapshot(
          `"<span {...___VERTER___ctx.bar}/>"`
        );
      });

      it("binding with :bind", () => {
        const { result } = transpile(`<span :bind="bar"/>`);

        expect(result).toMatchInlineSnapshot(
          `"<span bind={___VERTER___ctx.bar}/>"`
        );
      });
      it("binding multi-line", () => {
        const { result } = transpile(`<span :bind="
            i == 1 ? false : true
            "/>`);
        expect(result).toMatchInlineSnapshot(`
          "<span bind={
                      ___VERTER___ctx.i == 1 ? false : true
                      }/>"
        `);
      });

      it("binding boolean", () => {
        const { result } = transpile(`<span :bind="false"/>`);

        expect(result).toMatchInlineSnapshot(`"<span bind={false}/>"`);
      });

      it("binding with v-bind:bind", () => {
        const { result } = transpile(`<span v-bind:bind="bar"/>`);

        expect(result).toMatchInlineSnapshot(
          `"<span bind={___VERTER___ctx.bar}/>"`
        );
      });

      it("v-bind + props", () => {
        const { result } = transpile(`<span v-bind="bar" foo="bar"/>`);

        expect(result).toMatchInlineSnapshot(
          `"<span {...___VERTER___ctx.bar} foo="bar"/>"`
        );
      });

      it("props + v-bind", () => {
        const { result } = transpile(`<span foo="bar" v-bind="bar"/>`);

        expect(result).toMatchInlineSnapshot(
          `"<span foo="bar" {...___VERTER___ctx.bar}/>"`
        );
      });

      it("props + binding on array", () => {
        const { result } = transpile(`<span :foo="[bar]" />`);

        expect(result).toMatchInlineSnapshot(
          `"<span foo={[___VERTER___ctx.bar]} />"`
        );
      });

      it("props + binding complex", () => {
        const { result } = transpile(
          `<span :foo="isFoo ? { myFoo: foo } : undefined" />`
        );

        expect(result).toMatchInlineSnapshot(
          `"<span foo={___VERTER___ctx.isFoo ? { myFoo: ___VERTER___ctx.foo } : undefined} />"`
        );
      });

      it("should keep casing", () => {
        const { result } = transpile(`<span aria-autocomplete="bar"/>`);

        expect(result).toMatchInlineSnapshot(
          `"<span aria-autocomplete="bar"/>"`
        );
      });

      it("should keep casing on binding", () => {
        const { result } = transpile(`<span :aria-autocomplete="'bar'"/>`);

        expect(result).toMatchInlineSnapshot(
          `"<span aria-autocomplete={'bar'}/>"`
        );
      });

      it("should pass boolean on just props", () => {
        const { result } = transpile(`<span foo/>`);

        expect(result).toMatchInlineSnapshot(`"<span foo/>"`);
      });

      it("should not camelCase props", () => {
        const { result } = transpile(`<span supa-awesome-prop="hello"></span>`);

        expect(result).toMatchInlineSnapshot(
          `"<span supa-awesome-prop="hello"></span>"`
        );
      });

      it("v-bind", () => {
        const { result } = transpile(`<div v-bind="props" />`);

        expect(result).toMatchInlineSnapshot(
          `"<div {...___VERTER___ctx.props} />"`
        );
      });
      it("v-bind name", () => {
        const { result } = transpile(`<div v-bind:name="props" />`);

        expect(result).toMatchInlineSnapshot(
          `"<div name={___VERTER___ctx.props} />"`
        );
      });

      it("v-bind : without name", () => {
        const { result } = transpile(`<div : />`);

        expect(result).toMatchInlineSnapshot(`"<div {} />"`);
      });

      it("v-bind :short", () => {
        const { result } = transpile(`<div :name />`);

        expect(result).toMatchInlineSnapshot(
          `"<div name={___VERTER___ctx.name} />"`
        );
      });
      it("v-bind :short camelise", () => {
        const { result } = transpile(`<MyComp :foo-bar />`);

        expect(result).toMatchInlineSnapshot(
          `"<___VERTER___comp.MyComp fooBar={___VERTER___ctx.fooBar} />"`
        );
      });

      it("v-bind :shorter", () => {
        const { result } = transpile(`<div :name="name" />`);

        expect(result).toMatchInlineSnapshot(
          `"<div name={___VERTER___ctx.name} />"`
        );
      });

      it("bind arrow function", () => {
        const { result } = transpile(`<div :name="()=>name" />`);

        expect(result).toMatchInlineSnapshot(
          `"<div name={()=>___VERTER___ctx.name} />"`
        );
      });

      it("bind arrow function with return ", () => {
        const { result } = transpile(`<div :name="()=>{ return name }" />`);

        expect(result).toMatchInlineSnapshot(
          `"<div name={()=>{ return ___VERTER___ctx.name }} />"`
        );
      });

      it("bind with ?.", () => {
        const { result } = transpile(`<div :name="test?.random" />`);

        expect(result).toMatchInlineSnapshot(
          `"<div name={___VERTER___ctx.test?.random} />"`
        );
      });

      it("should append ctx inside of functions", () => {
        const { result } = transpile(
          `<span :check-for-something="e=> { foo = e }"></span>`
        );

        expect(result).toMatchInlineSnapshot(
          `"<span checkForSomething={e=> { ___VERTER___ctx.foo = e }}></span>"`
        );
      });

      it("should  append ctx inside a string interpolation", () => {
        const { result } = transpile(
          '<span :check-for-something="`foo=${bar}`"></span>'
        );

        expect(result).toMatchInlineSnapshot(
          `"<span checkForSomething={\`foo=\${___VERTER___ctx.bar}\`}></span>"`
        );
      });

      describe("class & style merge", () => {
        it("should do class merge", () => {
          const { result } = transpile(
            `<span class="foo" :class="['hello']"></span>`
          );
          expect(result).toMatchInlineSnapshot(
            `"<span  class={___VERTER___normalizeClass([['hello'],"foo"])}></span>"`
          );
        });

        it("should do class merge on bind sugar short", () => {
          const { result } = transpile(`<span class="foo" :class></span>`);

          expect(result).toMatchInlineSnapshot(
            `"<span  class={___VERTER___normalizeClass([___VERTER___ctx.class},"foo"])></span>"`
          );
        });

        it("should do class merge with v-bind", () => {
          const { result } = transpile(
            `<span class="foo" :class="['hello']" v-bind:class="{'oi': true}"></span>`
          );
          expect(result).toMatchInlineSnapshot(
            `"<span  class={___VERTER___normalizeClass([['hello'],"foo",{'oi': true}])} ></span>"`
          );
        });

        it("should do class merge with v-bind with attributes in between", () => {
          const { result } = transpile(
            `<span class="foo" don-t :class="['hello']" v-bind:class="{'oi': true}"></span>`
          );

          expect(result).toMatchInlineSnapshot(
            `"<span  don-t class={___VERTER___normalizeClass([['hello'],"foo",{'oi': true}])} ></span>"`
          );
        });

        it("should still append context accessor", () => {
          const { result } = transpile(
            `<span :class="foo" don-t :class="['hello', bar]" v-bind:class="{'oi': true, sup}"></span>`
          );

          expect(result).toMatchInlineSnapshot(
            `"<span class={___VERTER___normalizeClass([___VERTER___ctx.foo,['hello', ___VERTER___ctx.bar],{'oi': true, sup:___VERTER___ctx.sup}])} don-t  ></span>"`
          );
        });

        it("should do style merge", () => {
          const { result } = transpile(
            `<span style="foo" :style="['hello']"></span>`
          );

          expect(result).toMatchInlineSnapshot(
            `"<span  style={___VERTER___normalizeStyle([['hello'],"foo"])}></span>"`
          );
        });
        it("should do style merge with v-bind ", () => {
          const { result } = transpile(
            `<span style="foo" :style="['hello']" v-bind:style="{'oi': true}"></span>`
          );

          expect(result).toMatchInlineSnapshot(
            `"<span  style={___VERTER___normalizeStyle([['hello'],"foo",{'oi': true}])} ></span>"`
          );
        });
        it("should do style merge with v-bind with attributes in between", () => {
          const { result } = transpile(
            `<span style="foo" don-t :style="['hello']" v-bind:style="{'oi': true}"></span>`
          );

          expect(result).toMatchInlineSnapshot(
            `"<span  don-t style={___VERTER___normalizeStyle([['hello'],"foo",{'oi': true}])} ></span>"`
          );
        });

        it("should still append context accessor style", () => {
          const { result } = transpile(
            `<span :style="foo" don-t :style="['hello', bar]" v-bind:style="{'oi': true, sup}"></span>`
          );

          expect(result).toMatchInlineSnapshot(
            `"<span style={___VERTER___normalizeStyle([___VERTER___ctx.foo,['hello', ___VERTER___ctx.bar],{'oi': true, sup:___VERTER___ctx.sup}])} don-t  ></span>"`
          );
        });

        it("should append context on objects shothand", () => {
          const { result } = transpile(`<span :style="{colour}"></span>`);

          expect(result).toMatchInlineSnapshot(
            `"<span style={{colour:___VERTER___ctx.colour}}></span>"`
          );
        });

        it("should append context on objects", () => {
          const { result } = transpile(
            `<span :style="{colour: myColour}"></span>`
          );

          expect(result).toMatchInlineSnapshot(
            `"<span style={{colour: ___VERTER___ctx.myColour}}></span>"`
          );
        });

        it("should append context on dynamic accessor", () => {
          const { result } = transpile(
            `<span :style="{[colour]: true}"></span>`
          );

          expect(result).toMatchInlineSnapshot(
            `"<span style={{[___VERTER___ctx.colour]: true}}></span>"`
          );
        });

        it("should append context on dynamic accessor + value", () => {
          const { result } = transpile(
            `<span :style="{[colour]: myColour}"></span>`
          );

          expect(result).toMatchInlineSnapshot(
            `"<span style={{[___VERTER___ctx.colour]: ___VERTER___ctx.myColour}}></span>"`
          );
        });
      });
    });

    describe("on", () => {
      it("should add event correctly", () => {
        const { result } = transpile(`<span @back="navigateToSession(null)"/>`);

        expect(result).toMatchInlineSnapshot(
          `"<span onBack={(...args)=>___VERTER___eventCb(args,()=>___VERTER___ctx.navigateToSession(null))}/>"`
        );
      });
      it('should camelCase "on" event listeners', () => {
        const { result } = transpile(
          `<span @check-for-something="test"></span>`
        );

        expect(result).toMatchInlineSnapshot(
          `"<span onCheckForSomething={(...args)=>___VERTER___eventCb(args,()=>___VERTER___ctx.test)}></span>"`
        );
      });

      it("should append ctx inside of functions", () => {
        const { result } = transpile(
          `<span @check-for-something="e=> { foo = e }"></span>`
        );

        expect(result).toMatchInlineSnapshot(
          `"<span onCheckForSomething={(...args)=>___VERTER___eventCb(args,e=> { ___VERTER___ctx.foo = e })}></span>"`
        );
      });

      it("event should be ignored", () => {
        const { result } = transpile(
          `<span @back="navigateToSession($event)"/>`
        );

        expect(result).toMatchInlineSnapshot(
          `"<span onBack={(...args)=>___VERTER___eventCb(args,($event)=>___VERTER___ctx.navigateToSession($event))}/>"`
        );
      });

      it("empty @", () => {
        const { result } = transpile(`<span @></span>`);

        expect(result).toMatchInlineSnapshot(`"<span on></span>"`);
      });
    });

    describe("v-for", () => {
      it("simple", () => {
        const { result } = transpile(`<li v-for="item in items"></li>`);

        expect(result).toMatchInlineSnapshot(
          `"{___VERTER___renderList(___VERTER___ctx.items,item   =>{ <li ></li>})}"`
        );
      });

      it("destructing", () => {
        const { result } = transpile(`<li v-for="{ message } in items"></li>`);

        expect(result).toMatchInlineSnapshot(
          `"{___VERTER___renderList(___VERTER___ctx.items,({ message })   =>{ <li ></li>})}"`
        );
      });

      it("index", () => {
        const { result } = transpile(
          `<li v-for="(item, index) in items"></li>`
        );

        expect(result).toMatchInlineSnapshot(
          `"{___VERTER___renderList(___VERTER___ctx.items,(item, index)   =>{ <li ></li>})}"`
        );
      });

      it("index + key", () => {
        const { result } = transpile(
          `<li v-for="(item, index) in items" :key="index + 'random'"></li>`
        );

        expect(result).toMatchInlineSnapshot(
          `"{___VERTER___renderList(___VERTER___ctx.items,(item, index)   =>{ <li  key={index + 'random'}></li>})}"`
        );
      });

      it("destructing + index", () => {
        const { result } = transpile(
          `<li v-for="({ message }, index) in items"></li>`
        );

        expect(result).toMatchInlineSnapshot(
          `"{___VERTER___renderList(___VERTER___ctx.items,({ message }, index)   =>{ <li ></li>})}"`
        );
      });

      it("nested", () => {
        const { result } = transpile(`<li v-for="item in items">
        <span v-for="childItem in item.children"></span>
      </li>`);

        expect(result).toMatchInlineSnapshot(`
          "{___VERTER___renderList(___VERTER___ctx.items,item   =>{ <li >
                  {___VERTER___renderList(item.children,childItem   =>{ <span ></span>})}
                </li>})}"
        `);
      });

      it("of", () => {
        const { result } = transpile(`<li v-for="item of items"></li>`);

        expect(result).toMatchInlineSnapshot(
          `"{___VERTER___renderList(___VERTER___ctx.items,item   =>{ <li ></li>})}"`
        );
      });

      it("of with tab", () => {
        const { result } = transpile(`<li v-for="item   of items"></li>`);

        expect(result).toMatchInlineSnapshot(
          `"{___VERTER___renderList(___VERTER___ctx.items,item     =>{ <li ></li>})}"`
        );
      });

      it("of with tabs", () => {
        const { result } = transpile(`<li v-for="item   of     items"></li>`);

        expect(result).toMatchInlineSnapshot(
          `"{___VERTER___renderList(___VERTER___ctx.items,item         =>{ <li ></li>})}"`
        );
      });

      // object
      it("object", () => {
        const { result } = transpile(`<li v-for="value in myObject"></li>`);

        expect(result).toMatchInlineSnapshot(
          `"{___VERTER___renderList(___VERTER___ctx.myObject,value   =>{ <li ></li>})}"`
        );
      });

      it("object + key", () => {
        const { result } = transpile(
          `<li v-for="(value, key) in myObject"></li>`
        );

        expect(result).toMatchInlineSnapshot(
          `"{___VERTER___renderList(___VERTER___ctx.myObject,(value, key)   =>{ <li ></li>})}"`
        );
      });

      it("object + key + index", () => {
        const { result } = transpile(
          `<li v-for="(value, key, index) in myObject"></li>`
        );

        expect(result).toMatchInlineSnapshot(
          `"{___VERTER___renderList(___VERTER___ctx.myObject,(value, key, index)   =>{ <li ></li>})}"`
        );
      });

      // range
      it("range", () => {
        const { result } = transpile(`<li v-for="n in 10"></li>`);

        expect(result).toMatchInlineSnapshot(
          `"{___VERTER___renderList(10,n   =>{ <li ></li>})}"`
        );
      });

      // v-if has higher priority than v-for
      it("v-if", () => {
        const { result } = transpile(
          `<li v-for="i in items" v-if="items > 5"></li>`
        );

        expect(result).toMatchInlineSnapshot(`
          "{ ()=> {if(___VERTER___ctx.items > 5){{___VERTER___renderList(___VERTER___ctx.items,i   =>{ 
          if(!(___VERTER___ctx.items > 5)) { return; } <li  ></li>})}}}}"
        `);
      });

      it("should not append ctx to item.", () => {
        const { result } = transpile(`<li v-for="item in items">
            {{ item. }}            
            </li>`);

        expect(result).toMatchInlineSnapshot(`
          "{___VERTER___renderList(___VERTER___ctx.items,item   =>{ <li >
                      {{ item. }}            
                      </li>})}"
        `);
      });

      // TODO move this to transpile.spec.ts
      it.skip("should append ctx to item", () => {
        const { result } = transpile(`<li v-for="item in items">
            {{ foo. }}            
            </li>`);

        expect(result).toMatchInlineSnapshot(`
              "{___VERTER___renderList(___VERTER___ctx.items,(item)=>{<li >
                        { ___VERTER___ctx.foo. }            
                        </li>})}"
            `);
      });
    });

    describe("conditions", () => {
      it("v-if", () => {
        const { result } = transpile(`<li v-if="n > 5"></li>`);

        expect(result).toMatchInlineSnapshot(
          `"{ ()=> {if(___VERTER___ctx.n > 5){<li ></li>}}}"`
        );
      });

      it("v-if + v-else", () => {
        const { result } = transpile(
          `<li v-if="n > 5" id="if"></li><li v-else id="else"></li>`
        );
        expect(result).toMatchInlineSnapshot(
          `
          "{ ()=> {if(___VERTER___ctx.n > 5){<li  id="if"></li>}else{
          <li  id="else"></li>
          }}}"
        `
        );
      });

      it("v-if + v-else component", () => {
        const { result } = transpile(
          `<Comp v-if="n > 5" id="if"></Comp><Comp v-else id="else"></Comp>`
        );
        expect(result).toMatchInlineSnapshot(
          `
          "{ ()=> {if(___VERTER___ctx.n > 5){<___VERTER___comp.Comp  id="if"></___VERTER___comp.Comp>}else{
          <___VERTER___comp.Comp  id="else"></___VERTER___comp.Comp>
          }}}"
        `
        );
      });

      it("v-if + v-else-if", () => {
        const { result } = transpile(
          `<li v-if="n > 5"></li><li v-else-if="n > 3"></li>`
        );
        expect(result).toMatchInlineSnapshot(
          `"{ ()=> {if(___VERTER___ctx.n > 5){<li ></li>}else if(___VERTER___ctx.n > 3){<li ></li>}}}"`
        );
      });

      it("v-if + >", () => {
        const { result } = transpile(`<div v-if="getData.length > 0"> </div>`);

        expect(result).toMatchInlineSnapshot(
          `"{ ()=> {if(___VERTER___ctx.getData.length > 0){<div > </div>}}}"`
        );
      });

      it("multiple conditions", () => {
        const { result } = transpile(`
                <li v-if="n === 1"></li>
                <li v-else-if="n === 1"></li>
                <li v-else-if="n === 1"></li>
                <li v-else-if="n === 1"></li>
                <li v-else-if="n === 1"></li>
                <li v-else-if="n === 1"></li>
                <li v-else></li>`);

        expect(result).toMatchInlineSnapshot(`
          "
                          { ()=> {if(___VERTER___ctx.n === 1){<li ></li>}
                          else if(___VERTER___ctx.n === 1){<li ></li>}
                          else if(___VERTER___ctx.n === 1){<li ></li>}
                          else if(___VERTER___ctx.n === 1){<li ></li>}
                          else if(___VERTER___ctx.n === 1){<li ></li>}
                          else if(___VERTER___ctx.n === 1){<li ></li>}
                          else{
          <li ></li>
          }}}"
        `);
      });

      it("multiple real-case", () => {
        const { result } = transpile(`
        <div class="flex flex-row items-center pb-2.5">
        <img
          v-if="props.item.content.content.type === 3"
          class="mr-2 h-9 w-9 select-none"
          src="@/assets/exchangeorder-icon.svg"
        />
        <img
          v-else-if="props.item.content.content.type === 2"
          class="mr-2 h-9 w-9 select-none"
          src="@/assets/rechargeorder-icon.svg"
        />
        <span v-if="props.item.content.content.type === 3">发送兑换订单</span>
        <span v-else-if="props.item.content.content.type === 2"
          >发送充值订单</span
        >
      </div>
       `);

        expect(result).toMatchInlineSnapshot(`
          "
                  <div class="flex flex-row items-center pb-2.5">
                  { ()=> {if(___VERTER___ctx.props.item.content.content.type === 3){<img
                    
                    class="mr-2 h-9 w-9 select-none"
                    src="@/assets/exchangeorder-icon.svg"
                  />}
                  else if(___VERTER___ctx.props.item.content.content.type === 2){<img
                    
                    class="mr-2 h-9 w-9 select-none"
                    src="@/assets/rechargeorder-icon.svg"
                  />}}}
                  { ()=> {if(___VERTER___ctx.props.item.content.content.type === 3){<span >发送兑换订单</span>}
                  else if(___VERTER___ctx.props.item.content.content.type === 2){<span 
                    >发送充值订单</span
                  >}}}
                </div>
                 "
        `);
      });

      it("ttt", () => {
        const { result } =
          transpile(`<ChatBubble :item="item" justify contentCss="w-248x h-98.5x">
        <div @click="openModal" class="flex flex-col">
          <div class="flex flex-row items-center pb-2.5">
            <img
              v-if="props.item.content.content.type === 3"
              class="mr-2 h-9 w-9 select-none"
              src="@/assets/exchangeorder-icon.svg"
            />
            <img
              v-else-if="props.item.content.content.type === 2"
              class="mr-2 h-9 w-9 select-none"
              src="@/assets/rechargeorder-icon.svg"
            />
            <span v-if="props.item.content.content.type === 3">Order3</span>
            <span v-else-if="props.item.content.content.type === 2"
              >Order2</span
            >
          </div>
          <span
            class="flex h-9 items-center justify-between border-t border-solid border-neutral-300 pb-2 pt-2.5 text-sm text-neutral-900"
            >Order4
            <img class="select-none" src="@/assets/arrow-right-small.svg" />
          </span>
        </div>
      </ChatBubble>`);

        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.ChatBubble item={___VERTER___ctx.item} justify contentCss="w-248x h-98.5x" v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          <div onClick={(...args)=>___VERTER___eventCb(args,()=>___VERTER___ctx.openModal)} class="flex flex-col">
                    <div class="flex flex-row items-center pb-2.5">
                      { ()=> {if(___VERTER___ctx.props.item.content.content.type === 3){<img
                        
                        class="mr-2 h-9 w-9 select-none"
                        src="@/assets/exchangeorder-icon.svg"
                      />}
                      else if(___VERTER___ctx.props.item.content.content.type === 2){<img
                        
                        class="mr-2 h-9 w-9 select-none"
                        src="@/assets/rechargeorder-icon.svg"
                      />}}}
                      { ()=> {if(___VERTER___ctx.props.item.content.content.type === 3){<span >Order3</span>}
                      else if(___VERTER___ctx.props.item.content.content.type === 2){<span 
                        >Order2</span
                      >}}}
                    </div>
                    <span
                      class="flex h-9 items-center justify-between border-t border-solid border-neutral-300 pb-2 pt-2.5 text-sm text-neutral-900"
                      >Order4
                      <img class="select-none" src="@/assets/arrow-right-small.svg" />
                    </span>
                  </div>
          })}

          }}>
                  
                </___VERTER___comp.ChatBubble>"
        `);
      });

      it.skip("multiple real-case", () => {
        const { result } =
          transpile(`<div @click="openModal" class="flex flex-col">
        <div class="flex flex-row items-center pb-2.5">
          <img
            v-if="props.item.content.content.type === 3"
            class="mr-2 h-9 w-9 select-none"
            src="@/assets/exchangeorder-icon.svg"
          />
          <img
            v-else-if="props.item.content.content.type === 2"
            class="mr-2 h-9 w-9 select-none"
            src="@/assets/rechargeorder-icon.svg"
          />
          <span v-if="props.item.content.content.type === 3">发送兑换订单</span>
          <span v-else-if="props.item.content.content.type === 2"
            >发送充值订单</span
          >
        </div>
        <span
          class="flex h-9 items-center justify-between border-t border-solid border-neutral-300 pb-2 pt-2.5 text-sm text-neutral-900"
          >选择您要查询的订单
          <img class="select-none" src="@/assets/arrow-right-small.svg" />
        </span>
      </div>`);

        expect(result).toMatchInlineSnapshot(`
          ","flex flex-col","flex flex-row items-center pb-2.5","mr-2 h-9 w-9 select-none","mr-2 h-9 w-9 select-none","flex h-9 items-center justify-between border-t border-solid border-neutral-300 pb-2 pt-2.5 text-sm text-neutral-900","select-none"<div onClick={___VERTER___ctx.openModal} >
                  <div >
                    { ()=> {if(___VERTER___ctx.props.item.content.content.type === 3){<img
                      
                      
                      src="@/assets/exchangeorder-icon.svg"
                    />}
                    else if(___VERTER___ctx.props.item.content.content.type === 2)}}{<img
                      
                      
                      src="@/assets/rechargeorder-icon.svg"
                    />}
                    { ()=> {if(___VERTER___ctx.props.item.content.content.type === 3){<span >发送兑换订单</span>}
                    else if(___VERTER___ctx.props.item.content.content.type === 2){<span 
                      >发送充值订单</span
          }}}          >
                  </div>
                  <span
                    
                    >选择您要查询的订单
                    <img  src="@/assets/arrow-right-small.svg" />
                  </span>
                </div>"
        `);
      });

      it("should narrow", () => {
        const { result } = transpile(
          `<li v-if="n === true" :key="n"></li><li v-else :key="n"/>`
        );
        expect(result).toMatchInlineSnapshot(
          `
          "{ ()=> {if(___VERTER___ctx.n === true){<li  key={___VERTER___ctx.n}></li>}else{
          <li  key={___VERTER___ctx.n}/>
          }}}"
        `
        );
      });

      describe("narrow", () => {
        it("arrow function", () => {
          const { result } = transpile(
            `<li v-if="n.n.n === true" :onClick="()=> n.n.n === false ? 1 : undefined"></li><li v-else :onClick="()=> n.n.n === true ? 1 : undefined"/>`
          );

          // NOTE the resulted snapshot should give an error with typescript in the correct environment
          expect(result).toMatchInlineSnapshot(
            `
            "{ ()=> {if(___VERTER___ctx.n.n.n === true){<li  onClick={()=> !(___VERTER___ctx.n.n.n === true) ? undefined : ___VERTER___ctx.n.n.n === false ? 1 : undefined}></li>}else{
            <li  onClick={()=> ___VERTER___ctx.n.n.n === true ? undefined : ___VERTER___ctx.n.n.n === true ? 1 : undefined}/>
            }}}"
          `
          );
        });
        it("arrow function with return", () => {
          const { result } = transpile(
            `<li v-if="n.n.n === true" :onClick="()=> { return  n.n.n === false ? 1 : undefined }" ></li><li v-else :onClick="()=>{ return n.n.n === true ? 1 : undefined}"/>`
          );

          // NOTE the resulted snapshot should give an error with typescript in the correct environment
          expect(result).toMatchInlineSnapshot(
            `
            "{ ()=> {if(___VERTER___ctx.n.n.n === true){<li  onClick={()=> { if(!(___VERTER___ctx.n.n.n === true)) { return; } return  ___VERTER___ctx.n.n.n === false ? 1 : undefined }} ></li>}else{
            <li  onClick={()=>{ if(___VERTER___ctx.n.n.n === true) { return; } return ___VERTER___ctx.n.n.n === true ? 1 : undefined}}/>
            }}}"
          `
          );
        });

        it("function", () => {
          const { result } = transpile(
            `<li v-if="n.n.n === true" :onClick="function() { return  n.n.n === false ? 1 : undefined } "></li><li v-else :onClick="function(){ return n.n.n === true ? 1 : undefined}"/>`
          );

          // NOTE the resulted snapshot should give an error with typescript in the correct environment
          expect(result).toMatchInlineSnapshot(
            `
            "{ ()=> {if(___VERTER___ctx.n.n.n === true){<li  onClick={function() { if(!(___VERTER___ctx.n.n.n === true)) { return; } return  ___VERTER___ctx.n.n.n === false ? 1 : undefined } }></li>}else{
            <li  onClick={function(){ if(___VERTER___ctx.n.n.n === true) { return; } return ___VERTER___ctx.n.n.n === true ? 1 : undefined}}/>
            }}}"
          `
          );
        });

        // NOTE Maybe we could enable this behaviour, something to think about
        // it.only("narrow on conditional arrow function", () => {
        //   const { result } = transpile(
        //     `<li v-if="n.n.n === true" :onClick="n.n.a === true ? (()=> n.n.n === false || n.n.a === false) : undefined "></li>`
        //   );

        //   // NOTE the resulted snapshot should give an error with typescript in the correct environment
        //   expect(result).toMatchInlineSnapshot(
        //     `"{(___VERTER___ctx.n.n.n === true)?<li  onClick={___VERTER___ctx.n.n.a === true ? (()=> !((___VERTER___ctx.n.n.n === true) && ___VERTER___ctx.n.n.a === true) ? undefined : ___VERTER___ctx.n.n.n === false || ___VERTER___ctx.n.n.a === false) : undefined }></li> : undefined}"`
        //   );
        // });

        it("narrow with v-for", () => {
          /**
           * To test, check with
           * ```ts
           * declare const r: { n: true, items: string[] } | { n: false, items: number[] };
           * ```
           */
          const { result } = transpile(
            `<div v-for="item in r.items" v-if="r.n === false" :key="r.n === true ? 1 : false"></div>`
          );
          // NOTE the resulted snapshot should give an error with typescript in the correct environment
          expect(result).toMatchInlineSnapshot(
            `
            "{ ()=> {if(___VERTER___ctx.r.n === false){{___VERTER___renderList(___VERTER___ctx.r.items,item   =>{ 
            if(!(___VERTER___ctx.r.n === false)) { return; } <div   key={___VERTER___ctx.r.n === true ? 1 : false}></div>})}}}}"
          `
          );
        });
      });

      // TODO work on this conditions
      describe.skip("invalid conditions", () => {
        it("v-else without v-if", () => {
          const { result } = transpile(`<li v-else></li>`);
          // expect(() => build(doParseElement(source))).throw(
          //   "v-else or v-else-if must be preceded by v-if"
          // );
          expect(result).toMatchInlineSnapshot(
            `"{(___VERTER___ctx.items > 5)?__VERTER__renderList(___VERTER___ctx.items,(i)=>{<li  ></li>}) : undefined}"`
          );
        });

        // it("v-else-if without v-if", () => {
        //   const { result } = transpile(`<li v-else-if></li>`);
        //   // expect(() => build(doParseElement(source))).throw(
        //   //   "v-else or v-else-if must be preceded by v-if"
        //   // );
        //   const parsed = doParseContent(source);
        //   const { magicString } = process(parsed);
        //   expect(result).toMatchInlineSnapshot(
        //     `"{(___VERTER___ctx.items > 5)?__VERTER__renderList(___VERTER___ctx.items,(i)=>{<li  ></li>}) : undefined}"`
        //   );
        // });
        it("v-else after v-else", () => {
          const { result } = transpile(
            `<li v-if="true"></li><li v-else></li><li v-else></li>`
          );

          expect(result).toMatchInlineSnapshot(
            `"{(___VERTER___ctx.items > 5)?__VERTER__renderList(___VERTER___ctx.items,(i)=>{<li  ></li>}) : undefined}"`
          );
        });

        it("v-else-if after v-else", () => {
          const { result } = transpile(
            `<li v-if="true"></li><li v-else></li><li v-else></li>`
          );
          // expect(() => build(doParseElement(source))).throw(
          //   "v-else or v-else-if must be preceded by v-if"
          // );
          expect(result).toMatchInlineSnapshot(
            `"{(___VERTER___ctx.items > 5)?__VERTER__renderList(___VERTER___ctx.items,(i)=>{<li  ></li>}) : undefined}"`
          );
        });
      });
    });
  });

  describe("slot", () => {
    describe("self-closing", () => {
      it("parse slot", () => {
        const { result } = transpile(`<slot/>`);
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot.default);
          return <RENDER_SLOT/>}}"
        `);
      });

      it("parse slot with name", () => {
        const { result } = transpile(`<slot name="test"/>`);
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot["test"]);
          return <RENDER_SLOT />}}"
        `);
      });

      it("parse slot with name expression", () => {
        const { result } = transpile(`<slot :name="test"/>`);
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot[___VERTER___ctx.test]);
          return <RENDER_SLOT />}}"
        `);
      });

      it("with v-bind should be default", () => {
        const { result } = transpile(`<slot :[msg]="test"/>`);
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot.default);
          return <RENDER_SLOT [___VERTER___ctx.msg]={___VERTER___ctx.test}/>}}"
        `);
      });

      it("with v-if", () => {
        const { result } = transpile(`<slot v-if="false"/>`);
        expect(result).toMatchInlineSnapshot(`
          "{ ()=> {if(false){const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot.default);
          return <RENDER_SLOT />}}}"
        `);
      });

      it("with v-for", () => {
        const { result } = transpile(
          `<slot v-for="name in $slots" :name="name"/>`
        );
        expect(result).toMatchInlineSnapshot(`
          "{___VERTER___renderList(___VERTER___ctx.$slots,name   =>{ const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot[name]);
          return <RENDER_SLOT  />})}"
        `);
      });

      it("element + slot", () => {
        const { result } = transpile(`<my-comp><slot/></my-comp>`);
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComp v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          {()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot.default);
          return <RENDER_SLOT/>}}
          })}

          }}></___VERTER___comp.MyComp>"
        `);
      });
    });
    describe("with children", () => {
      it("parse slot", () => {
        const { result } = transpile(`<slot>{{ 'hello' }}</slot>`);
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot.default);
          return <RENDER_SLOT>{{ 'hello' }}</RENDER_SLOT>}}"
        `);
      });

      it("parse slot with name", () => {
        const { result } = transpile(`<slot name="test">{{ 'hello' }}</slot>`);
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot["test"]);
          return <RENDER_SLOT >{{ 'hello' }}</RENDER_SLOT>}}"
        `);
      });

      it("parse slot with name expression", () => {
        const { result } = transpile(`<slot :name="test">{{ 'hello' }}</slot>`);
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot[___VERTER___ctx.test]);
          return <RENDER_SLOT >{{ 'hello' }}</RENDER_SLOT>}}"
        `);
      });

      it("with v-bind should be default", () => {
        const { result } = transpile(
          `<slot :[msg]="test">{{ 'hello' }}</slot>`
        );
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot.default);
          return <RENDER_SLOT [___VERTER___ctx.msg]={___VERTER___ctx.test}>{{ 'hello' }}</RENDER_SLOT>}}"
        `);
      });

      it("with v-if", () => {
        const { result } = transpile(`<slot v-if="false">{{ 'hello' }}</slot>`);
        expect(result).toMatchInlineSnapshot(`
          "{ ()=> {if(false){const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot.default);
          return <RENDER_SLOT >{{ 'hello' }}</RENDER_SLOT>}}}"
        `);
      });

      it("with v-for", () => {
        const { result } = transpile(
          `<slot v-for="name in $slots" :name="name">{{ 'hello' }}</slot>`
        );
        expect(result).toMatchInlineSnapshot(`
          "{___VERTER___renderList(___VERTER___ctx.$slots,name   =>{ const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot[name]);
          return <RENDER_SLOT  >{{ 'hello' }}</RENDER_SLOT>})}"
        `);
      });

      it("nested + v-if", () => {
        const { result } = transpile(`<slot v-if="disableDrag" :name="selected">
  <slot :tab="item" />
</slot>`);
        expect(result).toMatchInlineSnapshot(`
          "{ ()=> {if(___VERTER___ctx.disableDrag){const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot[___VERTER___ctx.selected]);
          return <RENDER_SLOT  >
            {()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot.default);
          return <RENDER_SLOT tab={___VERTER___ctx.item} />}}
          </RENDER_SLOT>}}}"
        `);
      });

      it("nested + v-if + typescript", () => {
        const { result } =
          transpile(`<slot v-if="disableDrag" :name="selected as T">
  <slot :tab="item" />
</slot>`);
        expect(result).toMatchInlineSnapshot(`
          "{ ()=> {if(___VERTER___ctx.disableDrag){const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot[___VERTER___ctx.selected as T]);
          return <RENDER_SLOT  >
            {()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot.default);
          return <RENDER_SLOT tab={___VERTER___ctx.item} />}}
          </RENDER_SLOT>}}}"
        `);
      });

      it("nested + default", () => {
        const { result } = transpile(
          '<slot :name="`page-${p?.name || p}`" :page="p">\n' +
            '<slot v-bind="p" />\n' +
            "</slot>"
        );
        expect(result).toMatchInlineSnapshot(`
          "{()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot[\`page-\${___VERTER___ctx.p?.name || ___VERTER___ctx.p}\`]);
          return <RENDER_SLOT  page={___VERTER___ctx.p}>
          {()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot.default);
          return <RENDER_SLOT {...___VERTER___ctx.p} />}}
          </RENDER_SLOT>}}"
        `);
      });

      it("element + nested + default", () => {
        const { result } = transpile(
          '<my-comp><slot :name="`page-${p?.name || p}`" :page="p">\n' +
            '<slot v-bind="p" />\n' +
            "</slot></my-comp>"
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComp v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          {()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot[\`page-\${___VERTER___ctx.p?.name || ___VERTER___ctx.p}\`]);
          return <RENDER_SLOT  page={___VERTER___ctx.p}>
          {()=>{

          const RENDER_SLOT = ___VERTER___AssertAny(___VERTER___slot.default);
          return <RENDER_SLOT {...___VERTER___ctx.p} />}}
          </RENDER_SLOT>}}
          })}

          }}></___VERTER___comp.MyComp>"
        `);
      });

      it("named with a v-if child", () => {
        const { result } = transpile(`<MyComp>
      <template #foo>
         <div></div>
        <div v-if="foo.n">
          {{foo.n}}
        </div>
        <Comp></Comp>
      </template>
      </MyComp>`);
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComp v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.foo)(()=>{

          <___VERTER___template >
                   <div></div>
                  { ()=> {if(___VERTER___ctx.foo.n){<div >
                    {{foo.n}}
                  </div>}}}
                  <___VERTER___comp.Comp></___VERTER___comp.Comp>
                </___VERTER___template>
          })}

          }}>
                
                </___VERTER___comp.MyComp>"
        `);
      });
    });
  });

  describe("partial", () => {
    test("<d", () => {
      const { result } = transpile("<d");
      expect(result).toMatchInlineSnapshot(`"<___VERTER___comp.d"`);
    });
  });
});
