<script>
	let c = $state(0);
	$effect.pre(() => { console.log(c); });
</script>
<button onclick={() => c++}>{c}</button>
