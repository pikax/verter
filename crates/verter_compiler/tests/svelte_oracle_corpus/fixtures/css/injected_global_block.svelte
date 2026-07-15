<svelte:options css="injected" />

<div class="card">x</div>

<style>
	.card {
		color: blue;
	}

	:global {
		.theme {
			color: red;
		}
	}
</style>
