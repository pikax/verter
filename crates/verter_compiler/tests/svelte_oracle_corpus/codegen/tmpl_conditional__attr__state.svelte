<script>
	let n = $state(0);
</script>

<div onclick={() => n++} id="a {n ? 1 : 2} b"></div>
