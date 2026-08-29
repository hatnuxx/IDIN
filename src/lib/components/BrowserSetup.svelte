<script>
  import { t } from '$lib/i18n/i18n.svelte.js';
  import { invoke } from '@tauri-apps/api/core';

  let { open, onclose } = $props();

  let step = $state(1);
  let browser = $state('chrome');
  let result = $state(null);
  let extensionId = $state('');
  let busy = $state(false);
  let stagedPath = $state('');

  const browsers = [
    { id: 'chrome', name: 'Google Chrome', page: 'chrome://extensions' },
    { id: 'edge', name: 'Microsoft Edge', page: 'edge://extensions' },
    { id: 'firefox', name: 'Firefox', page: 'about:debugging#/runtime/this-firefox' },
  ];
  const current = $derived(browsers.find((b) => b.id === browser));

  async function install() {
    busy = true;
    try {
      const extPath = await invoke('stage_extension_folder');
      result = await invoke('setup_browser_integration', { extensionId: extensionId.trim() });
      stagedPath = extPath;
      step = 2;
    } catch (e) {
      result = { error: String(e) };
      step = 2;
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <div class="overlay" role="dialog" aria-modal="true">
    <div class="dialog">
      <button class="close" onclick={onclose} aria-label={t('browserSetup.done')}>✕</button>

      {#if step === 1}
        <div class="dialog-header">
          <span class="header-icon">🌐</span>
          <h3>{t('browserSetup.title')}</h3>
        </div>
        <p class="hint">{t('browserSetup.hint')}</p>

        <div class="pick">
          {#each browsers as b (b.id)}
            <button
              class="browser-btn"
              class:sel={browser === b.id}
              onclick={() => (browser = b.id)}
            >
              {b.name}
            </button>
          {/each}
        </div>

        <label class="extid">
          <span>{t('browserSetup.extidLabel')}</span>
          <input bind:value={extensionId} placeholder={t('browserSetup.extidPlaceholder')} />
        </label>

        <div class="actions">
          <button class="primary" disabled={busy} onclick={install}>
            {busy ? t('browserSetup.installing') : t('browserSetup.install')}
          </button>
        </div>
      {:else}
        {#if result?.error}
          <div class="dialog-header">
            <span class="header-icon">⚠️</span>
            <h3>{t('browserSetup.error')}</h3>
          </div>
          <p class="err">{result.error}</p>
        {:else}
          <div class="dialog-header">
            <span class="header-icon">✅</span>
            <h3>{t('browserSetup.almostDone')}</h3>
          </div>
          <p class="hint">
            {t('browserSetup.registeredFor')}
            <strong>{result?.registered?.join('، ')}</strong>.
            {t('browserSetup.nowJust')}
          </p>
          <ol class="steps">
            <li>{t('browserSetup.step1')} <code>{current?.page}</code></li>
            <li>{t('browserSetup.step2')}</li>
            <li>{t('browserSetup.step3')} <code>{stagedPath}</code></li>
            <li>{t('browserSetup.step4')}</li>
          </ol>
        {/if}
        <div class="actions">
          <button class="ghost" onclick={() => (step = 1)}>{t('browserSetup.back')}</button>
          <button class="primary" onclick={onclose}>{t('browserSetup.done')}</button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(15, 18, 25, 0.65);
    backdrop-filter: blur(4px);
    display: grid;
    place-items: center;
    z-index: 60;
  }
  .dialog {
    width: min(520px, 92vw);
    background: var(--bg-card);
    border: 1px solid var(--stroke-strong);
    border-radius: var(--radius-lg, 14px);
    padding: 28px;
    box-shadow: var(--shadow);
    position: relative;
    animation: pop 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  @keyframes pop {
    from {
      transform: scale(0.92);
      opacity: 0;
    }
  }

  .close {
    position: absolute;
    top: 14px;
    inset-inline-end: 14px;
    color: var(--text-3);
    width: 28px;
    height: 28px;
    border-radius: 8px;
    transition: all 0.15s;
  }
  .close:hover {
    background: color-mix(in srgb, var(--danger) 12%, transparent);
    color: var(--danger);
  }

  .dialog-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }
  .header-icon {
    font-size: 18px;
  }
  h3 {
    font-size: 16px;
    color: var(--accent);
  }

  .hint {
    color: var(--text-2);
    font-size: 13px;
    line-height: 1.8;
  }

  .pick {
    display: flex;
    gap: 8px;
    margin: 18px 0;
    flex-wrap: wrap;
  }
  .browser-btn {
    padding: 10px 20px;
    border-radius: 10px;
    border: 1px solid var(--stroke);
    color: var(--text-2);
    transition: all 0.15s;
    font-size: 13px;
  }
  .browser-btn:hover {
    border-color: var(--stroke-strong);
    background: var(--accent-glow);
  }
  .browser-btn.sel {
    border-color: var(--accent);
    color: var(--accent);
    background: var(--accent-glow);
    font-weight: 600;
    box-shadow: 0 0 12px var(--accent-glow);
  }

  .extid {
    display: block;
    font-size: 12px;
    color: var(--text-3);
  }
  .extid input {
    width: 100%;
    margin-top: 6px;
    font: inherit;
    color: var(--text);
    background: var(--bg-hover);
    border: 1px solid var(--stroke);
    border-radius: 8px;
    padding: 8px 12px;
    direction: ltr;
    transition: border-color 0.15s;
  }
  .extid input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-glow);
  }

  .steps {
    margin: 14px 0;
    padding-inline-start: 20px;
    line-height: 2.1;
    font-size: 13px;
  }
  code {
    background: var(--bg-hover);
    padding: 2px 8px;
    border-radius: 6px;
    font-size: 12px;
    direction: ltr;
    display: inline-block;
    border: 1px solid var(--stroke);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
  }
  .primary {
    padding: 9px 22px;
    border-radius: 10px;
    background: linear-gradient(135deg, var(--accent), var(--accent-strong));
    color: #1b1b1b;
    font-weight: 700;
    transition: all 0.15s;
    box-shadow: 0 2px 12px var(--accent-glow);
  }
  .primary:hover {
    filter: brightness(1.1);
    transform: translateY(-1px);
  }
  .primary:disabled {
    opacity: 0.5;
    cursor: wait;
    transform: none;
  }
  .ghost {
    padding: 9px 18px;
    border-radius: 10px;
    border: 1px solid var(--stroke);
    color: var(--text-2);
    transition: all 0.15s;
  }
  .ghost:hover {
    border-color: var(--stroke-strong);
    color: var(--text);
  }
  .err {
    color: var(--danger);
    font-size: 13px;
    line-height: 1.6;
  }
</style>
