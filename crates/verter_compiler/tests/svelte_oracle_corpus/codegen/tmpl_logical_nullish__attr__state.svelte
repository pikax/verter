<script>
	let n = $state(0);
</script>

<div onclick={() => n++} id="a {n ?? 1} b"></div>
