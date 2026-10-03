<script lang="ts">
  import { clock, promo } from "./rofl.svelte";
</script>

<!-- A joke: a Russian-only "sale". -->
<section class="promo">
  <h2>КУПИТЬ ЕРП ПАНЕЛЬ</h2>
  <p class="line">
    УСПЕЙ КУПИТЬ! АКЦИЯ ДЛИТСЯ ЕЩЁ
    <b class="timer" class:over={promo.left !== null && promo.left <= 0}>
      {promo.left === null ? "--:--:--" : promo.left <= 0 ? "ЗАКОНЧИЛАСЬ" : clock(promo.left)}
    </b>
  </p>
  <p class="line big">СКИДКА 90 ПРОЦЕНТОВ В ЧЕСТЬ ОТКРЫТИЯ!</p>
  <div><button class="btn primary buy" onclick={() => (promo.open = true)}>КУПИТЬ</button></div>
</section>

<style>
  /* The same card as the settings sections, tinted. */
  .promo {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
    max-width: 720px;
    padding: 16px 18px 18px;
    border: 1px solid color-mix(in srgb, var(--warn) 45%, transparent);
    border-radius: 12px;
    background: color-mix(in srgb, var(--warn) 8%, transparent);
  }
  h2 {
    color: var(--bad);
    letter-spacing: 0.03em;
  }
  .line {
    margin: 0;
    font-weight: 600;
  }
  .big {
    font-size: 17px;
    color: var(--warn);
  }
  .timer {
    margin-left: 6px;
    padding: 1px 8px;
    border-radius: var(--radius-sm);
    background: var(--tint-hover);
    color: var(--bad);
    font-variant-numeric: tabular-nums;
  }
  .timer.over {
    color: var(--sub);
  }
  .buy {
    padding: 0 28px;
    min-height: 36px;
    letter-spacing: 0.05em;
  }
</style>
