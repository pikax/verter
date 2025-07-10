// @ts-nocheck
import { Component } from "vue"

type EnforceTypeChildren<T> = T extends HTMLElement ? [T, {}, JSX.Element] :
    T extends Component ? T extends {
        new(): {}
    }
    ? [T, {}, JSX.Element]
    : T extends Function ? [T, {}, JSX.Element]
    : T extends [infer Props, infer Type] ?
    [Type, Props, JSX.Element]
    : [
        any, T, JSX.Element
    ] : []


declare function RenderSlot<T extends Record<string, (...args: any[]) => any>, N extends keyof T, R extends ReturnType<T[N]>>(slots: T, name: N, render: R extends Array<infer C> ?
    Array<[C, {}, JSX.Element]> : [[
        R, {}, JSX.Element
    ]]): R;



declare function ForceRenderSlot<T extends Record<string, (...args: any[]) => any>, N extends keyof T, R extends ReturnType<T[N]>>(slots: T, name: N, render: R extends Array<infer C> ?
    Array<EnforceTypeChildren<C>> : [EnforceTypeChildren<R>]): R extends Component ? R extends {
        new(): {}
    } ? 1 : T extends Function ? 2 : 3 : 0;






declare const mySlots: {
    default: (a: { foo: string }) => typeof Comp,
    foo: (a: { foo: string }) => HTMLInputElement,
    // bar: (a: {}) => {test: string },
    // baz: () => [{test: string }, typeof Comp]
}

const r = RenderSlot(mySlots, 'default', [[Comp, {}, <Comp />], [Comp, {}, <div />]])

const d = ForceRenderSlot(mySlots, 'default', [[Comp, { name: 'test' }, <input name="test" />]])

// @ts-expect-error
ForceRenderSlot(mySlots, 'default', [[new HTMLElementTagNameMap['input'], { name: 'test' }, <input name="test" />]])


const e = ForceRenderSlot(mySlots, 'foo', ['input', { name: 'test' }, <input name="test" />]])
// @ts-expect-error
ForceRenderSlot(mySlots, 'foo', [[Comp, { name: 'test' }, <input name="test" />]])

const b = ForceRenderSlot(mySlots, 'bar', [[new HTMLInputElement, { test: 'test' }, <input name="test" />]])

// @ts-expect-error incorrect prop
ForceRenderSlot(mySlots, 'bar', [[new HTMLInputElement, { name: 'test' }, <input name="test" />]])

const z = ForceRenderSlot(mySlots, 'baz', [[Comp, { name: 'test' }, <input name="test" />]])

// @ts-expect-error invalid prop
ForceRenderSlot(mySlots, 'baz', [[Comp, { name: 'test' }, <input name="test" />]])
// @ts-expect-error invalid compoennt
ForceRenderSlot(mySlots, 'baz', [[new HTMLInputElement, { test: 'test' }, <input name="test" />]])

const iconSlots = defineSlots<[["icon", {}]]>()



const $slots = defineSlots<{
    default: () => HTMLDivElement[],

    input: (a: { name: string }) => HTMLInputElement
}>()

declare function RenderSlotDeclaration<T extends Record<string, (...args: any[]) => any>, K extends keyof T>(slots: T, name: K, o: [T[K] extends infer F ? F extends (arg: infer A extends {}) => infer R ? [A, R extends Array<any> ? R : [R]] : undefined : undefined]): any

RenderSlotDeclaration($slots, 'default', [
    [{}, [new HTMLDivElement()]]
])

RenderSlotDeclaration($slots, 'input', [
    [{ name: 'test' }, [new HTMLInputElement]]
])

$slots.default()


