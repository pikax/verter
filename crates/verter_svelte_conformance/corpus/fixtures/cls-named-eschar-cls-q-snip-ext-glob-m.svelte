{#snippet subject()}
	<div class="a a&amp;b">x</div>
{/snippet}

{@render subject()}

<style>
	:global(.a\&b) {
		color: red;
	}
</style>
