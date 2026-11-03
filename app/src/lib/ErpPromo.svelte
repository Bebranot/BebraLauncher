<script lang="ts">
  import { clock, promo } from "./rofl.svelte";
</script>

<!-- A joke: a Russian-only "sale", shown with a 1-in-5 chance until purchased. -->
{#if promo.visible && !promo.dismissedUntilRestart}
<section class="promo">
  <div class="head">
    <h2>ЕРП ПАНЕЛЬ «PREMIUM»</h2>
    <span class="age">18+</span>
  </div>
  <p class="line">
    УСПЕЙ КУПИТЬ! АКЦИЯ ДЛИТСЯ ЕЩЁ
    <b class="timer" class:over={promo.left !== null && promo.left <= 0}>
      {promo.left === null ? "--:--:--" : promo.left <= 0 ? "ЗАКОНЧИЛАСЬ" : clock(promo.left)}
    </b>
  </p>
  <div class="price">
    <s>1 490 ₽</s>
    <b>149 ₽</b>
    <span>/ мес</span>
    <em>−90%</em>
  </div>
  <p class="line big">СКИДКА 90 ПРОЦЕНТОВ В ЧЕСТЬ ОТКРЫТИЯ!</p>
  <div><button class="btn primary buy" onclick={() => (promo.open = true)}>КУПИТЬ ЗА 149 ₽</button></div>
</section>
{/if}

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
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  h2 {
    color: var(--bad);
    letter-spacing: 0.03em;
  }
  .age {
    display: grid;
    place-items: center;
    min-width: 30px;
    height: 22px;
    padding: 0 5px;
    border: 1px solid var(--bad);
    border-radius: 4px;
    color: var(--bad);
    font-size: 11px;
    font-weight: 700;
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
  .price {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .price s {
    color: var(--sub);
    font-size: 15px;
  }
  .price b {
    color: var(--fg);
    font-size: 24px;
    font-variant-numeric: tabular-nums;
  }
  .price span {
    color: var(--sub);
    font-size: 12px;
  }
  .price em {
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    background: var(--bad);
    color: #ffffff;
    font-size: 11px;
    font-style: normal;
    font-weight: 700;
  }
  .buy {
    padding: 0 28px;
    min-height: 36px;
    letter-spacing: 0.05em;
  }
</style>
