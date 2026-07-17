<script>
  let lastPointerId = $state(0);

  // @ts-expect-error unannotated JavaScript named handlers intentionally require authored JSDoc
  function myClick(e) {
    lastPointerId = e.pointerId;
  }
</script>

<button onpointerdown={myClick}>Click</button>
<output>{lastPointerId}</output>
