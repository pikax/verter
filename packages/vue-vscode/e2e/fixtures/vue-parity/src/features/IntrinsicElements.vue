<script setup lang="ts">
 /**
 * Native HTML/SVG intrinsic elements for element-interface hover / type tests.
 * Each open tag carries a unique data-intrinsic marker so anchors are unambiguous.
 */
</script>

<template>
  <section data-intrinsic="section-root">
    <div data-intrinsic="div-tag" class="intrinsic-div">div</div>
    <span data-intrinsic="span-tag">span</span>
    <button data-intrinsic="button-tag" type="button" disabled>btn</button>
    <input data-intrinsic="input-tag" type="text" />
    <a data-intrinsic="a-tag" href="https://example.com/intrinsic-a">link</a>
    <img data-intrinsic="img-tag" alt="intrinsic-img" src="x.png" />
    <form data-intrinsic="form-tag" action="/intrinsic-form"></form>
    <select data-intrinsic="select-tag">
      <option value="1">one</option>
    </select>
    <textarea data-intrinsic="textarea-tag"></textarea>
    <h1 data-intrinsic="h1-tag">heading</h1>
    <p data-intrinsic="p-tag">para</p>
    <ul data-intrinsic="ul-tag">
      <li data-intrinsic="li-tag">item</li>
    </ul>
    <svg data-intrinsic="svg-tag" width="10" height="10"></svg>
  </section>
</template>
