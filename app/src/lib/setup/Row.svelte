<script lang="ts">
  // A setting that is not a switch: the name and a line of explanation, the control beside it.
  import type { Snippet } from "svelte";

  let {
    label,
    hint = "",
    children,
  }: { label: string; hint?: string; children: Snippet } = $props();
</script>

<div class="row">
  <div class="text">
    <span class="label">{label}</span>
    {#if hint}<small class="sub">{hint}</small>{/if}
  </div>
  <div class="control">{@render children()}</div>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 8px 16px;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1 1 240px;
    min-width: 0;
  }
  small {
    font-size: 13px;
    line-height: 1.35;
  }
  .control {
    flex: 0 1 auto;
    min-width: 0;
  }
  .control :global(select),
  .control :global(input[type="text"]) {
    max-width: 100%;
  }
</style>
