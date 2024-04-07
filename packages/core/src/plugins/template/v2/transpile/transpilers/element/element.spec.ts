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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          <___VERTER___comp.MySpan v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.name)((props)=>{
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots[___VERTER___ctx.bar])((props)=>{
          <___VERTER___template />
          })}

          }}></___VERTER___comp.MyComponent>"
        `);
      });

      it("with slot comment", () => {
        const { result } = transpile(
          `<my-component>
          <!-- test -->
          <template #default/>
        </my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          <!-- test -->
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
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
          <!-- test -->
          <template #default/>
        </my-component>`
        );
        expect(result).toMatchInlineSnapshot(`
          "<___VERTER___comp.MyComponent v-slot={(ComponentInstance)=>{
          const $slots = ComponentInstance.$slots;
          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{


          {___VERTER___SLOT_CALLBACK($slots.default)(()=>{

          <!-- test -->
                    <___VERTER___template />
          })}
          <span>{{test}}</span>
          })}

          }}>
                    
                    
                  </___VERTER___comp.MyComponent>"
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
});
