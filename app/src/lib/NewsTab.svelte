<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { api } from "./api";
  import { formatDate } from "./format";
  import { i18n, t } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import { localized, unseen } from "./news";
  import { app, type NewsSource } from "./store.svelte";

  const SOURCES: { id: NewsSource; label: string }[] = [
    { id: "official", label: "Space Station 14" },
    { id: "own", label: "BebraLauncher" },
  ];

  // Read the feed once, when the tab is shown. Not an `$effect`: that re-ran on every state
  // change and turned a failed request into an endless loop of requests.
  onMount(() => {
    if (app.ownNewsUnseen > 0) app.newsSource = "own";
    void app.loadNews();
    void app.loadOwnNews();
  });

  // What was new when the list was opened stays marked until the tab is left.
  let fresh = $state<string[]>([]);
  $effect(() => {
    if (app.newsSource !== "own" || app.ownNews.state !== "ready") return;
    const items = app.ownNews.items;
    untrack(() => {
      const added = unseen(items, app.seenOwnNews).map((item) => item.id);
      if (added.length > 0) fresh = [...fresh, ...added];
      app.markOwnNewsSeen();
    });
  });

  /** The feed gives RFC 822 dates; show them as the player's own date format. */
  function shortDate(text: string): string {
    const d = Date.parse(text);
    return Number.isNaN(d) ? "" : new Date(d).toLocaleDateString();
  }
</script>

<div class="tab" data-scroll>
  <div class="sources" role="group" aria-label={t("news.sources")}>
    {#each SOURCES as source (source.id)}
      <button
        class:on={app.newsSource === source.id}
        aria-pressed={app.newsSource === source.id}
        onclick={() => (app.newsSource = source.id)}
      >
        {source.label}
        {#if source.id === "own" && app.ownNewsUnseen > 0}<i class="dot" aria-hidden="true"></i>{/if}
      </button>
    {/each}
  </div>

  {#if app.newsSource === "official"}
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
  {:else if app.ownNews.state === "loading" || app.ownNews.state === "idle"}
    <p class="sub">{t("news.loading")}</p>
  {:else if app.ownNews.state === "failed"}
    <p class="error">{t("news.failed", { error: app.ownNews.error })}</p>
    <button class="btn" onclick={() => app.loadOwnNews(true)}>{t("servers.refresh")}</button>
  {:else if app.ownNews.items.length === 0}
    <p class="sub">{t("news.ownEmpty")}</p>
  {:else}
    <div class="articles">
      {#each app.ownNews.items as item (item.id)}
        <article>
          <header>
            <h2>{localized(item.title, i18n.lang)}</h2>
            {#if fresh.includes(item.id)}<span class="new">{t("news.new")}</span>{/if}
            {#if item.date}
              <time class="sub date" datetime={item.date}>{formatDate(`${item.date}T12:00:00`)}</time>
            {/if}
          </header>
          {#if localized(item.body, i18n.lang)}
            <p class="body">{localized(item.body, i18n.lang)}</p>
          {/if}
          {#if item.link}
            {@const link = item.link}
            <button class="more link" onclick={() => api.openUrl(link)} title={link}>
              {t("news.more")}
              <Icon name="external" size={14} />
            </button>
          {/if}
        </article>
      {/each}
    </div>
  {/if}
</div>

<style>
  .tab {
    height: 100%;
    overflow-y: auto;
    padding: 12px 18px 20px;
  }
  .sources {
    display: inline-flex;
    gap: 2px;
    padding: 3px;
    border-radius: var(--radius);
    background: var(--tint);
  }
  .sources button {
    appearance: none;
    position: relative;
    display: flex;
    align-items: center;
    gap: 7px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    padding: 6px 14px;
    color: var(--sub);
    font: inherit;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition:
      background 0.12s ease,
      color 0.12s ease;
  }
  .sources button:hover {
    color: var(--fg);
  }
  .sources button.on {
    background: var(--tint-hover);
    color: var(--fg);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }
  ul {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  li {
    display: flex;
    gap: 14px;
    align-items: baseline;
    justify-content: space-between;
    padding: 10px 14px;
    border-radius: var(--radius);
    background: var(--tint);
    transition: background 0.12s;
  }
  li:hover {
    background: var(--tint-hover);
  }
  li .link {
    color: var(--fg);
    font-weight: 500;
  }
  li .link:hover {
    color: var(--accent);
    text-decoration: none;
  }
  .date {
    font-size: 13px;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  p {
    margin: 12px 0;
  }
  .articles {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 12px;
  }
  article {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    padding: 14px 16px;
    border-radius: var(--radius);
    background: var(--tint);
  }
  article header {
    display: flex;
    align-items: baseline;
    gap: 10px;
    width: 100%;
  }
  article h2 {
    font-size: 15px;
    line-height: 1.3;
    text-wrap: balance;
    user-select: text;
  }
  article .date {
    margin-left: auto;
  }
  .new {
    flex: none;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--fg);
    font-size: 12px;
    font-weight: 500;
  }
  .body {
    margin: 0;
    max-width: 70ch;
    line-height: 1.5;
    white-space: pre-line;
    user-select: text;
  }
  .more {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
</style>
