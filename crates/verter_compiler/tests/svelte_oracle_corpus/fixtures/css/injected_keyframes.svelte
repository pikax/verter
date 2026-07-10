<svelte:options css="injected" />

<div class="spinner">x</div>

<style>
	.spinner {
		animation: spin 1s linear infinite;
	}

	@keyframes spin {
		from {
			transform: rotate(0deg);
		}
		to {
			transform: rotate(360deg);
		}
	}

	@keyframes -global-fade {
		from {
			opacity: 0;
		}
	}
</style>
