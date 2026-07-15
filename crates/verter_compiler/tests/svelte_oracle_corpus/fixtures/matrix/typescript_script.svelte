<script lang="ts">
	import type { Hidden } from "./types.js";
	import { type Meta, seed } from "./values.js";

	interface Model {
		value: string;
	}

	type Count = number;
	const identity = <T,>(value: T): T => value;
	let count: Count = $state(identity(seed as number satisfies number));
	let form: Model = { value: "" };

	function advance(delta: number = 1): void {
		count! += delta!;
		count!++;
	}
</script>

<input bind:value={form!.value} />
<button onclick={advance}>{count!}</button>
