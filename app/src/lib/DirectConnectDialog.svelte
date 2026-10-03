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
    if (!address.trim() || app.busy) return;
    onclose();
    void app.connectTo(address);
  }
</script>

<Modal title={t("direct.title")} {onclose}>
  <form onsubmit={submit}>
    <label for="direct-address">{t("direct.text")}</label>
    <input
      id="direct-address"
      type="text"
      placeholder={t("direct.placeholder")}
      bind:value={address}
      bind:this={input}
      spellcheck="false"
      autocomplete="off"
    />
    <small class="sub">{t("direct.hint")}</small>
    <div>
      <button class="btn" type="submit" disabled={!address.trim() || app.busy}>
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
    font-size: 13px;
  }
</style>
