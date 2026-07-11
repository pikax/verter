<script>
	let open = $state(true);
	let { flag } = $props();
</script>

{#if open}
	<div class={flag ? 'a b' : '\u0062'}>x</div>
{/if}

<style>
	[data-x="zz"] {
		color: red;
	}
</style>
