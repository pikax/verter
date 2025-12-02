<script setup lang="ts">
// Whitespace and formatting edge cases

const   spacedVar   =   1   ;

const multiline = {
  a: 1,
  
  
  b: 2,
  
  c: 3
};

function 
spacedFunction
(
  arg1: string,
  arg2: number
)
: 
string 
{
  return arg1 + arg2;
}

// Tab indentation
const tabbed = {
	a: 1,
	b: 2,
};

// Mixed indentation
const mixed = {
  a: 1,
	b: 2,
    c: 3,
};

// Trailing whitespace intentionally left   
const trailing = "value";    

// Empty lines




const afterEmpty = 1;

// Very long line that might cause issues with some parsers or formatters because it exceeds typical line length limits and contains a lot of content in a single statement
const veryLongVariable = "This is a very long string value that continues for quite a while and might cause issues with line wrapping or parsing in some edge cases";

// Carriage returns (Windows line endings) might be normalized
const crlfTest = "test\r\nvalue";
</script>

<template>
  <div>
    <p>   Whitespace before and after   </p>
    <p>
      
      Multiple blank lines
      
    </p>
    <p>		Tab indented content</p>
  </div>
</template>
