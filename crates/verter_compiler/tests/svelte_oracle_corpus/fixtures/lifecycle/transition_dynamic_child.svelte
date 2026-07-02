<script>
	let c = $state(0);
</script>

<div transition:fade onclick={() => c++}><span>{c}</span></div>
