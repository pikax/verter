<script>
	let c = $state(0);
</script>

<div use:foo onclick={() => c++}>{#if c}<span>{c}</span>{/if}</div>
