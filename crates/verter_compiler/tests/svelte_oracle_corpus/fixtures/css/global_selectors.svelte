<div class="card"><span class="x">a</span></div>

<style>
	.card {
		color: blue;
	}

	:global(.x) {
		color: green;
	}

	:global {
		.y {
			color: red;
		}
	}
</style>
