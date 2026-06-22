<script>
	let n = $state(0);
</script>

<div onclick={() => n++} id={String.raw`x${n}y`}></div>
