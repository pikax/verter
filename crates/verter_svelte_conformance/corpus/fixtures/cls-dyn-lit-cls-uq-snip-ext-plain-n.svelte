<script>
	let { flag } = $props();
</script>

{#snippet subject()}
	<div class={flag ? 'a b' : '\u0062'}>x</div>
{/snippet}

{@render subject()}

<style>
	.zz {
		color: red;
	}
</style>
