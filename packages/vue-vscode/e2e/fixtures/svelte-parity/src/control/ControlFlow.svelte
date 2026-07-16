<script lang="ts">
  interface User {
    name: string;
    email: string;
  }

  let users: User[] = [
    { name: "Ada", email: "ada@example.com" },
    { name: "Grace", email: "grace@example.com" },
  ];
  let selected: User | null = null;
</script>

<div>
  {#each users as user (user.email)}
    <button onclick={() => (selected = user)}>{user.name}</button>
  {/each}
  {#if selected}
    <p>{selected.name} &lt;{selected.email}&gt;</p>
  {:else}
    <p>none</p>
  {/if}
</div>
