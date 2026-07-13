{#snippet subject()}
	<div class="a a&amp;b&#38;c">x</div>
{/snippet}

{@render subject()}

<style>
	:global(.a\26 b\&c) {
		color: red;
	}
</style>
