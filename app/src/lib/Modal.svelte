<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    onclose,
    children,
  }: { title: string; onclose: () => void; children: Snippet } = $props();
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="modal" role="dialog" aria-modal="true" aria-label={title}>
    <header>
      <h2>{title}</h2>
      <button class="close" onclick={onclose} aria-label="Закрыть">✕</button>
    </header>
    <div class="body">{@render children()}</div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: grid;
    place-items: center;
    z-index: 10;
  }
  .modal {
    width: min(480px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    display: flex;
    flex-direction: column;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  .close {
    border: none;
    background: none;
    padding: 4px 8px;
  }
  .body {
    padding: 16px;
    overflow: auto;
  }
</style>
