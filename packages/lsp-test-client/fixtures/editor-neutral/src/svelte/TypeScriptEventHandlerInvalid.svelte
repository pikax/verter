<script lang="ts">
  function myClick(e) {
    return e.__verterMissingPointerMember;
  }
</script>

<button onclick={myClick}>Click</button>
