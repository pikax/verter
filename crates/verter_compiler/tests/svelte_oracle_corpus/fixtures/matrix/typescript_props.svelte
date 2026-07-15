<script lang="ts">
	const uid: string = $props.id(), local: number = 1 as number;
	let { count = 1 as number, enabled = true satisfies boolean } = $props();
</script>

<p>{uid}</p>
<p>{count}</p>
<p>{enabled}</p>
