<script setup lang="ts">
// Special characters in strings and identifiers

// Escape sequences in strings
const backslash = "\\";
const quote = "\"";
const singleQuote = '\'';
const backtick = `\``;
const newline = "line1\nline2";
const tab = "col1\tcol2";
const carriage = "line1\rline2";
const unicode = "\u0048\u0065\u006C\u006C\u006F"; // "Hello"
const hex = "\x48\x65\x6C\x6C\x6F"; // "Hello"
const emoji = "👋🌍🎉";
const zwj = "👨‍👩‍👧‍👦"; // Family emoji with ZWJ

// Template literal edge cases
const templateWithExpression = `${1 + 2}`;
const templateWithBacktick = `Contains \` backtick`;
const templateWithDollar = `Price: $100 and ${50}`;
const templateNested = `outer ${`inner ${1}`}`;
const templateMultiline = `
  Line 1
  Line 2
  Line 3
`;

// Tagged template with special chars
function tag(strings: TemplateStringsArray, ...values: unknown[]) {
  return strings.join("");
}
const tagged = tag`Hello ${"world"}!`;

// String with HTML-like content
const htmlLike = "<div class=\"test\">Content</div>";
const scriptLike = "script-tag-content";
const styleLike = "style-tag-content";

// Regex with special characters
const regex1 = /[a-z]+/gi;
const regex2 = /\d{3}-\d{4}/;
const regex3 = /^start.*end$/m;
const regex4 = /[\u0000-\u007F]/;
const regexWithSlash = /https?:\/\//;
const regexWithQuotes = /["'`]/g;

// Object keys with special characters
const obj = {
  "kebab-case": 1,
  "with spaces": 2,
  "with\"quotes": 3,
  123: 4,
  [Symbol.iterator]: function* () { yield 1; },
};

// Comments with special characters (already parsed)
// various special chars: quotes, backticks, escapes

// URL in string
const url = "https://example.com/path?query=value&other=123#hash";

// JSON in string
const json = '{"key": "value", "array": [1, 2, 3]}';

// Base64 in string
const base64 = "SGVsbG8gV29ybGQh";

// Very long string that might stress line handling
const longString = "A".repeat(1000);
</script>

<template>
  <div>
    <p>Emoji: {{ emoji }}</p>
    <p>Unicode: {{ unicode }}</p>
    <p v-html="htmlLike"></p>
  </div>
</template>
