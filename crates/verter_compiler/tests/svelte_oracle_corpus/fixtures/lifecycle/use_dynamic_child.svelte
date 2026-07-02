<script>
	let c = $state(0);
</script>

<div use:foo onclick={() => c++}><span>{c}</span></div>
