<script>
  let { foo } = $props();
  function act() {}
</script>
<div use:act="{foo}">hi</div>
