<script>export let obj;</script>
{#each obj.list() as item}{@const y = obj.m()}<p>{y}</p>{/each}
