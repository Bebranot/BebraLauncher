<script lang="ts">
  import { api } from "./api";
  import { t } from "./i18n.svelte";
  import { app } from "./store.svelte";

  // Read the feed when the tab is first shown.
  $effect(() => {
    void app.loadNews();
  });

  /** The feed gives RFC 822 dates; show them as the player's own date format. */
  function shortDate(text: string): string {
    const d = Date.parse(text);
    return Number.isNaN(d) ? "" : new Date(d).toLocaleDateString();
  }
</script>

<div class="tab">
  <h2>{t("news.recent")}</h2>

  {#if app.news.state === "loading" || app.news.state === "idle"}
    <p class="sub">{t("news.loading")}</p>
  {:else if app.news.state === "failed"}
    <p class="error">{t("news.failed", { error: app.news.error })}</p>
    <button class="btn" onclick={() => app.loadNews(true)}>{t("servers.refresh")}</button>
  {:else if app.news.items.length === 0}
    <p class="sub">{t("news.empty")}</p>
  {:else}
    <ul>
      {#each app.news.items as item (item.link)}
        <li>
          <button class="link" onclick={() => api.openUrl(item.link)} title={item.link}>
            {item.title}
          </button>
          {#if shortDate(item.date)}<span class="sub date">{shortDate(item.date)}</span>{/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .tab {
    height: 100%;
    overflow-y: auto;
    padding: 12px 18px;
  }
  ul {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  li {
    display: flex;
    gap: 14px;
    align-items: baseline;
  }
  .date {
    font-size: 13px;
    white-space: nowrap;
  }
  p {
    margin: 12px 0;
  }
</style>
