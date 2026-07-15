<script>
	let open = $state(true);
	let { value } = $props();
</script>

{#if open}
	<div class={value}>x</div>
{/if}

<style>
	.b {
		color: red;
	}
</style>
