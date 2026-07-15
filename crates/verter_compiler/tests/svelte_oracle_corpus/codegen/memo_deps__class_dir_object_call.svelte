<script>
	let x = $state(0);
</script>

<div class:on={Boolean(x)} onclick={() => x++}></div>
