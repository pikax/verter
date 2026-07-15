<script>export let obj;</script>
{#each obj.m() as item (item.id)}<p>{item}</p>{/each}
