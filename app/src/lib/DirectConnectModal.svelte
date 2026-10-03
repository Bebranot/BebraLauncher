<script lang="ts">
  import { t } from "./i18n.svelte";
  import Modal from "./Modal.svelte";
  import { app } from "./store.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let address = $state("");
  let input = $state<HTMLInputElement>();

  $effect(() => input?.focus());

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!address.trim()) return;
    onclose();
    void app.connectTo(address);
  }
</script>

<Modal title={t("direct.title")} {onclose}>
  <form onsubmit={submit}>
    <input
      type="text"
      placeholder={t("direct.placeholder")}
      bind:value={address}
      bind:this={input}
      spellcheck="false"
      autocomplete="off"
    />
    <small class="dim">{t("direct.hint")}</small>
    <div>
      <button class="primary" type="submit" disabled={!address.trim() || app.busy}>
        {t("direct.connect")}
      </button>
    </div>
  </form>
</Modal>

<style>
  form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  small {
    font-size: 12px;
  }
</style>
