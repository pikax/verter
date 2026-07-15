<div class="spinner">x</div>

<style>
	@keyframes spin {
		from {
			opacity: 0;
		}
		to {
			opacity: 1;
		}
	}

	@keyframes -global-fade {
		from {
			opacity: 0;
		}
	}

	.spinner {
		animation: spin 1s linear infinite;
		animation-name: spin;
	}
</style>
