<script>
	let editable = $state('true');
	let hidden = $state(false);
</script>

<div onclick={() => editable = 'false'} contenteditable={editable}></div>
<button onclick={() => hidden = !hidden} hidden={hidden}></button>
