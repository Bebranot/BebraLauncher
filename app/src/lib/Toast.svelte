<script lang="ts">
  import { t } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./store.svelte";

  const running = $derived(app.play.phase === "running" ? app.play : null);
</script>

{#if running || app.notice}
  <div class="toast" role="status">
    {#if running}
      <span class="good">●</span>
      <span>{t("game.running", { name: running.playsAs })}</span>
    {:else if app.notice}
      <span>{app.notice}</span>
      <button class="x" aria-label={t("game.dismiss")} onclick={() => (app.notice = null)}>
        <Icon name="close" size={16} />
      </button>
    {/if}
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    left: 10px;
    bottom: 46px;
    z-index: 25;
    max-width: min(560px, calc(100vw - 20px));
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    background: var(--popup);
    border: 1px solid var(--ctrl-mid);
    border-left: 3px solid var(--accent);
    box-shadow: 0 4px 14px #000a;
  }
  .x {
    appearance: none;
    background: none;
    border: 0;
    padding: 0;
    cursor: pointer;
    color: var(--sub);
    display: grid;
  }
  .x:hover {
    color: var(--fg);
  }
</style>
