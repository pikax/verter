<script lang="ts">
  let query: string = $state('');
  let selected: number = $state(1);
  let enabled: boolean = $state(true);
</script>

<div class="toolbar">
  <div>
    Filter panels
    <input bind:value={query} />
  </div>
  <button onclick={() => enabled = !enabled}>Toggle dashboard</button>
  <p>Selected panel: {selected}</p>
</div>

{#if enabled}
  <div class="dashboard">
    <div class="panel" class:active={selected === 1} data-panel="1">
      <h2>Panel 1</h2>
      <p>{query}</p>
      <button onclick={() => selected = 1}>Select panel 1</button>
    </div>
    <div class="panel" class:active={selected === 2} data-panel="2">
      <h2>Panel 2</h2>
      <p>{query}</p>
      <button onclick={() => selected = 2}>Select panel 2</button>
    </div>
    <div class="panel" class:active={selected === 3} data-panel="3">
      <h2>Panel 3</h2>
      <p>{query}</p>
      <button onclick={() => selected = 3}>Select panel 3</button>
    </div>
    <div class="panel" class:active={selected === 4} data-panel="4">
      <h2>Panel 4</h2>
      <p>{query}</p>
      <button onclick={() => selected = 4}>Select panel 4</button>
    </div>
    <div class="panel" class:active={selected === 5} data-panel="5">
      <h2>Panel 5</h2>
      <p>{query}</p>
      <button onclick={() => selected = 5}>Select panel 5</button>
    </div>
    <div class="panel" class:active={selected === 6} data-panel="6">
      <h2>Panel 6</h2>
      <p>{query}</p>
      <button onclick={() => selected = 6}>Select panel 6</button>
    </div>
    <div class="panel" class:active={selected === 7} data-panel="7">
      <h2>Panel 7</h2>
      <p>{query}</p>
      <button onclick={() => selected = 7}>Select panel 7</button>
    </div>
    <div class="panel" class:active={selected === 8} data-panel="8">
      <h2>Panel 8</h2>
      <p>{query}</p>
      <button onclick={() => selected = 8}>Select panel 8</button>
    </div>
    <div class="panel" class:active={selected === 9} data-panel="9">
      <h2>Panel 9</h2>
      <p>{query}</p>
      <button onclick={() => selected = 9}>Select panel 9</button>
    </div>
    <div class="panel" class:active={selected === 10} data-panel="10">
      <h2>Panel 10</h2>
      <p>{query}</p>
      <button onclick={() => selected = 10}>Select panel 10</button>
    </div>
    <div class="panel" class:active={selected === 11} data-panel="11">
      <h2>Panel 11</h2>
      <p>{query}</p>
      <button onclick={() => selected = 11}>Select panel 11</button>
    </div>
    <div class="panel" class:active={selected === 12} data-panel="12">
      <h2>Panel 12</h2>
      <p>{query}</p>
      <button onclick={() => selected = 12}>Select panel 12</button>
    </div>
    <div class="panel" class:active={selected === 13} data-panel="13">
      <h2>Panel 13</h2>
      <p>{query}</p>
      <button onclick={() => selected = 13}>Select panel 13</button>
    </div>
    <div class="panel" class:active={selected === 14} data-panel="14">
      <h2>Panel 14</h2>
      <p>{query}</p>
      <button onclick={() => selected = 14}>Select panel 14</button>
    </div>
    <div class="panel" class:active={selected === 15} data-panel="15">
      <h2>Panel 15</h2>
      <p>{query}</p>
      <button onclick={() => selected = 15}>Select panel 15</button>
    </div>
    <div class="panel" class:active={selected === 16} data-panel="16">
      <h2>Panel 16</h2>
      <p>{query}</p>
      <button onclick={() => selected = 16}>Select panel 16</button>
    </div>
    <div class="panel" class:active={selected === 17} data-panel="17">
      <h2>Panel 17</h2>
      <p>{query}</p>
      <button onclick={() => selected = 17}>Select panel 17</button>
    </div>
    <div class="panel" class:active={selected === 18} data-panel="18">
      <h2>Panel 18</h2>
      <p>{query}</p>
      <button onclick={() => selected = 18}>Select panel 18</button>
    </div>
    <div class="panel" class:active={selected === 19} data-panel="19">
      <h2>Panel 19</h2>
      <p>{query}</p>
      <button onclick={() => selected = 19}>Select panel 19</button>
    </div>
    <div class="panel" class:active={selected === 20} data-panel="20">
      <h2>Panel 20</h2>
      <p>{query}</p>
      <button onclick={() => selected = 20}>Select panel 20</button>
    </div>
    <div class="panel" class:active={selected === 21} data-panel="21">
      <h2>Panel 21</h2>
      <p>{query}</p>
      <button onclick={() => selected = 21}>Select panel 21</button>
    </div>
    <div class="panel" class:active={selected === 22} data-panel="22">
      <h2>Panel 22</h2>
      <p>{query}</p>
      <button onclick={() => selected = 22}>Select panel 22</button>
    </div>
    <div class="panel" class:active={selected === 23} data-panel="23">
      <h2>Panel 23</h2>
      <p>{query}</p>
      <button onclick={() => selected = 23}>Select panel 23</button>
    </div>
    <div class="panel" class:active={selected === 24} data-panel="24">
      <h2>Panel 24</h2>
      <p>{query}</p>
      <button onclick={() => selected = 24}>Select panel 24</button>
    </div>
    <div class="panel" class:active={selected === 25} data-panel="25">
      <h2>Panel 25</h2>
      <p>{query}</p>
      <button onclick={() => selected = 25}>Select panel 25</button>
    </div>
    <div class="panel" class:active={selected === 26} data-panel="26">
      <h2>Panel 26</h2>
      <p>{query}</p>
      <button onclick={() => selected = 26}>Select panel 26</button>
    </div>
    <div class="panel" class:active={selected === 27} data-panel="27">
      <h2>Panel 27</h2>
      <p>{query}</p>
      <button onclick={() => selected = 27}>Select panel 27</button>
    </div>
    <div class="panel" class:active={selected === 28} data-panel="28">
      <h2>Panel 28</h2>
      <p>{query}</p>
      <button onclick={() => selected = 28}>Select panel 28</button>
    </div>
    <div class="panel" class:active={selected === 29} data-panel="29">
      <h2>Panel 29</h2>
      <p>{query}</p>
      <button onclick={() => selected = 29}>Select panel 29</button>
    </div>
    <div class="panel" class:active={selected === 30} data-panel="30">
      <h2>Panel 30</h2>
      <p>{query}</p>
      <button onclick={() => selected = 30}>Select panel 30</button>
    </div>
    <div class="panel" class:active={selected === 31} data-panel="31">
      <h2>Panel 31</h2>
      <p>{query}</p>
      <button onclick={() => selected = 31}>Select panel 31</button>
    </div>
    <div class="panel" class:active={selected === 32} data-panel="32">
      <h2>Panel 32</h2>
      <p>{query}</p>
      <button onclick={() => selected = 32}>Select panel 32</button>
    </div>
  </div>
{:else}
  <p class="disabled">Dashboard disabled</p>
{/if}

<style>
  .toolbar {
    display: grid;
    gap: 0.75rem;
    grid-template-columns: minmax(16rem, 1fr) auto auto;
    align-items: end;
    padding: 1rem;
  }

  .dashboard {
    display: grid;
    grid-template-columns: repeat(4, minmax(12rem, 1fr));
    gap: 1rem;
    padding: 1rem;
  }

  .panel {
    border: 1px solid #cbd5e1;
    border-radius: 0.5rem;
    padding: 1rem;
  }

  .panel.active {
    border-color: #2563eb;
    background: #eff6ff;
  }

  .disabled {
    color: #64748b;
    padding: 2rem;
  }
</style>
