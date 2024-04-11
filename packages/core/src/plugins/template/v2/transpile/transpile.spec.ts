import { MagicString, parse as sfcParse } from "@vue/compiler-sfc";
import { transpile } from "./";

describe("transpile", () => {
  function doTranspile(content: string) {
    const source = `<template>${content}</template>`;

    const sfc = sfcParse(source);

    const template = sfc.descriptor.template;
    const ast = template.ast!;
    const s = new MagicString(ast.source);
    const c = transpile(ast, s);

    return {
      sfc,
      s,
      c,

      original: s.original,
      result: s.toString(),
    };
  }

  describe("simple", () => {
    it("root", () => {
      const { result } = doTranspile("");

      expect(result).toMatchInlineSnapshot(`"<></>"`);
    });

    it("Hello vue", () => {
      const source = `<div>Hello vue</div>`;
      const { result } = doTranspile(source);

      expect(result).toMatchInlineSnapshot(`"<><div>{ "Hello vue" }</div></>"`);
    });

    it("component self closing", () => {
      const source = `<my-component/>`;

      const { result } = doTranspile(source);

      expect(result).toMatchInlineSnapshot(
        `"<><___VERTER___comp.MyComponent/></>"`
      );
    });

    it("comment", () => {
      const source = `<!-- this is comment -->`;

      const { result } = doTranspile(source);

      expect(result).toMatchInlineSnapshot(`"<>{/* this is comment */}</>"`);
    });

    it("v-if with expression", () => {
      const { result } = doTranspile(`<div v-if="(() => {
            let ii = '0';
            return ii === ii
          })()">
            t4est
          </div>
          <div v-else>
            else
          </div>`);

      expect(result).toMatchInlineSnapshot(`
        "<>{ ()=> {if((() => {
                    let ii = '0';
                    return ii === ii
                  })()){<div >{ " t4est " }</div>}
                  else{
        <div >{ " else " }</div>
        }}}</>"
      `);
    });

    it.only("test", () => {
      const { result } = doTranspile(`<script setup lang="ts">
      import ChatBubble from "./../ChatBubble.vue";
      import type { AppMessage } from "@/utils/message";
      import ChatTextHighlight from "./../ChatTextHighlight.vue";
      
      defineProps<{
        item: AppMessage<"NORPLAY_USERMESSAGE_BEYONGD_THREE_MINUTES">;
        sessionId: string;
      }>();
      </script>
      
      <template>
        <ChatBubble :item="item" justify>
          <ChatTextHighlight
            text="NORPLAY_USERMESSAGE_BEYONGD_THREE_MINUTES"
            :session-id="sessionId"
          />
        </ChatBubble>
      </template>
      `);
    });

    it.skip("complex ", () => {
      const { result } =
        doTranspile(`<div v-if="isSingle" class="h-full w-full">
          <MediaPreview
            ref="currentPreviewEl"
            :message="mediaMessages[currentIndex]"
            :src="src"
            :type="type"
            :preview="preview"
          />
        </div>
    
        <div v-else ref="swiperContainerEl" class="swiper flex h-full w-full">
          <div v-show="!isPinched" class="absolute flex h-full w-full items-center">
            <div ref="prevEl" class="button-prev absolute z-10">
              <svg
                xmlns="http://www.w3.org/2000/svg"
                width="20"
                height="20"
                viewBox="0 0 20 20"
                fill="white"
              >
                <path
                  d="M10.4772727,0.477272727 C10.7408632,0.740863176 10.7408632,1.16822773 10.4772727,1.43181818 L1.90909091,10 L10.4772727,18.5681818 C10.7408632,18.8317723 10.7408632,19.2591368 10.4772727,19.5227273 C10.2136823,19.7863177 9.78631772,19.7863177 9.52272727,19.5227273 L0.707106781,10.7071068 C0.316582489,10.3165825 0.316582489,9.68341751 0.707106781,9.29289322 L9.52272727,0.477272727 C9.78631772,0.213682278 10.2136823,0.213682278 10.4772727,0.477272727 Z"
                  transform="translate(4)"
                ></path>
              </svg>
            </div>
            <div :class="nextEl" class="button-next absolute right-0 z-10">
              <svg
                xmlns="http://www.w3.org/2000/svg"
                width="20"
                height="20"
                viewBox="0 0 20 20"
                fill="white"
              >
                <path
                  d="M1.37727273,19.5227273 C1.11368228,19.2591368 1.11368228,18.8317723 1.37727273,18.5681818 L9.94545455,10 L1.37727273,1.43181818 C1.11368228,1.16822773 1.11368228,0.740863176 1.37727273,0.477272727 C1.64086318,0.213682278 2.06822773,0.213682278 2.33181818,0.477272727 L11.1474387,9.29289322 C11.537963,9.68341751 11.537963,10.3165825 11.1474387,10.7071068 L2.33181818,19.5227273 C2.06822773,19.7863177 1.64086318,19.7863177 1.37727273,19.5227273 Z"
                  transform="translate(4)"
                ></path>
              </svg>
            </div>
          </div>
          <div class="swiper-wrapper">
            <div
              v-for="(item, i) in mediaMessages"
              :key="\`preview-\${item.id}\`"
              class="swiper-slide items-center justify-center"
              style="display: flex"
            >
              <div class="swiper-zoom-container mx-6 flex h-full w-full">
                <MediaPreview
                  :ref="
                    i === currentIndex ? (e) => (currentPreviewEl = e) : undefined
                  "
                  class="swiper-zoom-target"
                  :message="item"
                  :selected="i === currentIndex"
                  :noRender="shouldNotPreload(i)"
                />
              </div>
            </div>
          </div>
        </div>`);

      expect(result).toMatchInlineSnapshot(`
            "{(___VERTER___ctx.isSingle)?<div  class="h-full w-full">
                        <___VERTER__comp.MediaPreview
                          ref="currentPreviewEl"
                          message={___VERTER___ctx.mediaMessages[___VERTER___ctx.currentIndex]}
                          src={___VERTER___ctx.src}
                          type={___VERTER___ctx.type}
                          preview={___VERTER___ctx.preview}
                        />
                      </div>
                  
                      :<div  ref="swiperContainerEl" class="swiper flex h-full w-full">
                        <div v-show="!isPinched" class="absolute flex h-full w-full items-center">
                          <div ref="prevEl" class="button-prev absolute z-10">
                            <svg
                              xmlns="http://www.w3.org/2000/svg"
                              width="20"
                              height="20"
                              viewBox="0 0 20 20"
                              fill="white"
                            >
                              <path
                                d="M10.4772727,0.477272727 C10.7408632,0.740863176 10.7408632,1.16822773 10.4772727,1.43181818 L1.90909091,10 L10.4772727,18.5681818 C10.7408632,18.8317723 10.7408632,19.2591368 10.4772727,19.5227273 C10.2136823,19.7863177 9.78631772,19.7863177 9.52272727,19.5227273 L0.707106781,10.7071068 C0.316582489,10.3165825 0.316582489,9.68341751 0.707106781,9.29289322 L9.52272727,0.477272727 C9.78631772,0.213682278 10.2136823,0.213682278 10.4772727,0.477272727 Z"
                                transform="translate(4)"
                              ></path>
                            </svg>
                          </div>
                          <div class={__VERTER__normalizeClass([___VERTER___ctx.nextEl,"button-next absolute right-0 z-10"])} >
                            <svg
                              xmlns="http://www.w3.org/2000/svg"
                              width="20"
                              height="20"
                              viewBox="0 0 20 20"
                              fill="white"
                            >
                              <path
                                d="M1.37727273,19.5227273 C1.11368228,19.2591368 1.11368228,18.8317723 1.37727273,18.5681818 L9.94545455,10 L1.37727273,1.43181818 C1.11368228,1.16822773 1.11368228,0.740863176 1.37727273,0.477272727 C1.64086318,0.213682278 2.06822773,0.213682278 2.33181818,0.477272727 L11.1474387,9.29289322 C11.537963,9.68341751 11.537963,10.3165825 11.1474387,10.7071068 L2.33181818,19.5227273 C2.06822773,19.7863177 1.64086318,19.7863177 1.37727273,19.5227273 Z"
                                transform="translate(4)"
                              ></path>
                            </svg>
                          </div>
                        </div>
                        <div class="swiper-wrapper">
                          {__VERTER__renderList(___VERTER___ctx.mediaMessages,(item, i)=>{if((___VERTER___ctx.isSingle)) { return; } <div
                            
                            key={\`preview-\${item.id}\`}
                            class="swiper-slide items-center justify-center"
                            style="display: flex"
                          >
                            <div class="swiper-zoom-container mx-6 flex h-full w-full">
                              <___VERTER__comp.MediaPreview
                                ref={
                                  i === ___VERTER___ctx.currentIndex ? (e) => (!(i === ___VERTER___ctx.currentIndex) || (___VERTER___ctx.isSingle) ? undefined : ___VERTER___ctx.currentPreviewEl = e) : undefined
                                }
                                class="swiper-zoom-target"
                                message={item}
                                selected={i === ___VERTER___ctx.currentIndex}
                                noRender={___VERTER___ctx.shouldNotPreload(i)}
                              />
                            </div>
                          </div>})}
                        </div>
                      </div>}"
          `);
    });
  });
});
