{#snippet subject()}
	<div class="wrap">
	<div id=a&copy;b>x</div>
</div>
{/snippet}

{@render subject()}

<style>
	.wrap #a©b {
		color: red;
	}
</style>
