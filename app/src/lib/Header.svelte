<script lang="ts">
  import { api } from "./api";
  import AccountDropdown from "./AccountDropdown.svelte";
  import { t } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import TabBar from "./TabBar.svelte";

  // The same community links as the official launcher's header.
  const DISCORD = "https://discord.ss14.io/";
  const WEBSITE = "https://spacestation14.com";
</script>

<header>
  <Logo />
  <TabBar />
  <div class="right">
    <button
      class="btn ghost icon"
      title={t("header.discord")}
      aria-label={t("header.discord")}
      onclick={() => api.openUrl(DISCORD)}
    >
      <Icon name="chat" size={18} />
    </button>
    <button
      class="btn ghost icon"
      title={t("header.website")}
      aria-label={t("header.website")}
      onclick={() => api.openUrl(WEBSITE)}
    >
      <Icon name="web" size={18} />
    </button>
    <AccountDropdown />
  </div>
</header>

<style>
  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--line);
  }
  .right {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-left: auto;
    min-width: 0;
  }
</style>
