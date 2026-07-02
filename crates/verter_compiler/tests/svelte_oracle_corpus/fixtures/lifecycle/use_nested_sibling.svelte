<script>
	let c = $state(0);
</script>

<div onclick={() => c++}><span use:foo><span>{c}</span></span><span>{c}</span></div>
