<script>
	let n = $state(0);
</script>

<div onclick={() => n++} class="a {n ?? 1} b"></div>
