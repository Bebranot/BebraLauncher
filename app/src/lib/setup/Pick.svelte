<script lang="ts" generics="V extends string | number">
  // One choice out of a few, shown as tiles. Real radio buttons underneath, so the arrow keys
  // and screen readers work the way they do everywhere else.
  import type { Snippet } from "svelte";
  import Icon from "../Icon.svelte";

  interface Opt {
    value: V;
    label: string;
    hint?: string;
    /** A short mark on the tile, e.g. "fits this computer". */
    badge?: string;
  }

  let {
    name,
    legend,
    options,
    value,
    onpick,
    min = 150,
    lead,
  }: {
    name: string;
    legend: string;
    options: Opt[];
    value: V;
    onpick: (value: V) => void;
    /** Narrowest a tile gets, in px. */
    min?: number;
    /** Something drawn above the label (a colour sample, say). */
    lead?: Snippet<[Opt]>;
  } = $props();
</script>

<fieldset class="pick-group" style:--pick-min="{min}px">
  <legend class="sr">{legend}</legend>
  {#each options as o (o.value)}
    <label class="pick" class:on={o.value === value}>
      <input
        class="sr"
        type="radio"
        {name}
        value={o.value}
        checked={o.value === value}
        onchange={() => onpick(o.value)}
      />
      {#if lead}<span class="lead">{@render lead(o)}</span>{/if}
      <span class="head">
        <span class="label">{o.label}</span>
        {#if o.value === value}<span class="mark"><Icon name="check" size={16} /></span>{/if}
      </span>
      {#if o.badge}<span class="badge">{o.badge}</span>{/if}
      {#if o.hint}<span class="hint">{o.hint}</span>{/if}
    </label>
  {/each}
</fieldset>

<style>
  .pick-group {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(var(--pick-min), 1fr));
    gap: 8px;
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
    border: 0;
  }
  .pick {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    padding: 10px 12px;
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    cursor: pointer;
    transition:
      background 0.12s,
      border-color 0.12s;
  }
  .pick:hover {
    background: var(--tint-hover);
  }
  .pick.on {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .pick:has(input:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .label {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .mark {
    display: grid;
    color: var(--accent);
  }
  .badge {
    align-self: flex-start;
    padding: 1px 8px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 600;
    background: var(--accent);
    color: var(--on-accent);
  }
  .hint {
    font-size: 13px;
    line-height: 1.4;
    color: var(--sub);
  }
  .lead {
    display: block;
  }
</style>
