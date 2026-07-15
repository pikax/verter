<script lang="ts">
	type Model = { value: number; nested: { count: number } };

	let {
		model = $bindable({ value: 0, nested: { count: 0 } }),
		plain = { value: 0 },
		key = 'value' as const
	}: { model?: Model; plain?: { value: number }; key?: 'value' } = $props();
</script>

<button onclick={() => model!.value++}>nonnull</button>
<button onfocus={() => (model as Model).nested.count = 1}>asserted</button>
<button onmouseenter={() => model[key as 'value'] += 1}>computed</button>
<button onmouseleave={() => plain!.value++}>plain</button>
