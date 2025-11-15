import { defineComponent } from "vue";

export type GetVueComponent<T> = T extends { new (): infer I }
  ? I
  : T extends (...args: any) => infer R
  ? void extends R
    ? typeof import("vue").Comment
    : R extends Array<any>
    ? typeof import("vue").Fragment
    : HTMLElement
  : T extends HTMLElement
  ? T
  : never;

/**
 * Helper to maintain the actual type of the name and inheritAttrs options
 * @param o
 */
export declare function DefineOptions<
  T extends { name?: TName; inheritAttrs?: TAttr },
  TName extends string,
  TAttr extends boolean
>(o: T): T;
