<script module lang="ts">
	import type { Hidden } from "./types.js";
	import config from "./config.json" assert { type: "json" };

	export interface PublicShape {
		value: number;
	}

	export const VERSION: number = 1;
	export const configured = config;
</script>

<script lang="ts">
	let count: number = $state(0 as number);

	function advance(): void {
		count += 1;
	}
</script>

<button onclick={(advance)}>{count!}</button>
