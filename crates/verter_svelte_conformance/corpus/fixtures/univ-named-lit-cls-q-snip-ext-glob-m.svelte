{#snippet subject()}
	<div class="a a&copy;b">x</div>
{/snippet}

{@render subject()}

<style>
	:global(*) {
		color: red;
	}
</style>
