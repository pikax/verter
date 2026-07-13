<script>
	let open = $state(true);
	let { flag } = $props();
</script>

{#if open}
	<div class="wrap">
	<div class={flag ? 'a b' : '\u0062'}>x</div>
</div>
{/if}

<style>
	.wrap p {
		color: red;
	}
</style>
