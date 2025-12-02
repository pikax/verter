<script setup lang="ts">
// Edge cases and unusual patterns for parser testing

import { ref, computed } from "vue";

// Unusual but valid variable names
const _underscore = ref(1);
const $dollar = ref(2);
const __ = ref(3);
const _$mixed$_ = ref(4);

// Unicode identifiers
const 变量 = ref("Chinese");
const переменная = ref("Russian");
const 変数 = ref("Japanese");

// Reserved word-like identifiers (valid in some contexts)
// const yield = 1; // Invalid
// const await = 2; // Invalid in async context
const _yield = ref(1);
const _await = ref(2);
const _async = ref(3);

// Number-prefixed (invalid, for error testing reference)
// const 1invalid = 1;

// Long identifiers
const veryLongVariableNameThatMightCauseIssuesWithSomeParserImplementations = ref("test");

// Empty destructuring
const { } = {};
const [] = [];

// Complex destructuring
const { a: { b: { c: deepNested } } } = { a: { b: { c: 1 } } };
const [first, , third, ...rest] = [1, 2, 3, 4, 5];
const { x = 10, y = 20 } = {};
const [head = "default", ...tail] = [];

// Computed property names
const propName = "dynamic";
const obj = {
  [propName]: 1,
  ["computed" + "Name"]: 2,
  [Symbol.iterator]: function* () { yield 1; },
};

// Getters and setters in object
const accessorObj = {
  _value: 0,
  get value() { return this._value; },
  set value(v: number) { this._value = v; },
};

// Method shorthand
const methods = {
  normal: function() {},
  shorthand() {},
  async asyncMethod() {},
  *generator() { yield 1; },
  async *asyncGenerator() { yield 1; },
};

// Tagged template literals
function tag(strings: TemplateStringsArray, ...values: any[]) {
  return strings.join("");
}
const tagged = tag`Hello ${"world"}!`;

// Optional chaining edge cases
const optionalCall = (null as any)?.method?.();
const optionalIndex = (null as any)?.[0]?.value;

// Nullish coalescing edge cases
const maybeNull: string | null = Math.random() > 0.5 ? "value" : null;
const nullish = maybeNull ?? "default";
const nested: string | undefined = Math.random() > 0.5 ? "value" : undefined;
const nestedNullish = nested ?? "fallback";

// Spread patterns
const spreadArray = [...[1, 2], ...[3, 4]];
const spreadObject = { ...{ a: 1 }, ...{ b: 2 }, ...{ a: 3 } };

// Arrow function edge cases
const noParams = () => 1;
const singleParam = (x: unknown) => x;
const destructuredParam = ({ a, b }: { a: number; b: number }) => a + b;
const restParam = (...args: number[]) => args.reduce((a, b) => a + b, 0);
const defaultParam = (x: number = 10) => x;
const thisBinding = function(this: { value: number }) { return this.value; };

// Immediately invoked function expressions
const iife = (() => "result")();
const asyncIife = (async () => "async result")();

// void operator
const voidResult = void 0;
const voidCall = void console.log("side effect");

// typeof in expression
const typeofString = typeof "hello";
const typeofSymbol = typeof Symbol();

// in operator
const hasProperty = "prop" in { prop: 1 };

// instanceof
const isArray = [] instanceof Array;

// Comma operator (with side effects to avoid warnings)
let commaCounter = 0;
const commaResult = (commaCounter++, commaCounter++, commaCounter);

// Binary and unary operators
const bitwiseAnd = 5 & 3;
const bitwiseOr = 5 | 3;
const bitwiseXor = 5 ^ 3;
const bitwiseNot = ~5;
const leftShift = 5 << 2;
const rightShift = 5 >> 2;
const unsignedRightShift = -5 >>> 2;

// Exponentiation
const power = 2 ** 10;

// Logical assignment
let logicalOr = 0;
logicalOr ||= 10;
let logicalAnd = 1;
logicalAnd &&= 20;
let nullishAssign: number | null = null;
nullishAssign ??= 30;

// Class expressions
const MyClass = class NamedClass {
  static staticProp = 1;
  instanceProp = 2;
  #privateField = 3;
  
  get #privateGetter() { return this.#privateField; }
  set #privateSetter(v: number) { this.#privateField = v; }
};

// BigInt
const bigInt = 9007199254740991n;
const bigIntOps = bigInt + 1n;

// Symbol
const sym = Symbol("description");
const symFor = Symbol.for("global");

// WeakMap / WeakSet
const weakMap = new WeakMap<object, string>();
const weakSet = new WeakSet<object>();

// Proxy
const proxy = new Proxy({}, {
  get(target, prop) { return prop; },
  set(target, prop, value) { return true; },
});

// Reflect
const reflected = Reflect.get({ a: 1 }, "a");

// RegExp
const regex = /pattern/gi;
const regexConstructor = new RegExp("pattern", "gi");
</script>

<template>
  <div>
    <h2>Edge Cases</h2>
    <p>Underscore: {{ _underscore }}</p>
    <p>Dollar: {{ $dollar }}</p>
    <p>Unicode: {{ 变量 }}</p>
    <p>Deep nested: {{ deepNested }}</p>
  </div>
</template>
