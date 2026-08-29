<script>
  import { t } from '$lib/i18n/i18n.svelte.js';

  let { task, onpause, onresume, onremove, onspeedlimit } = $props();

  const stateKey = $derived(`task.${task.state}`);
  const pct = $derived(task.progress ?? 0);
  let showSpeed = $state(false);
  let speedVal = $state('');

  function fmtBytes(n) {
    if (n == null) return '—';
    const u = ['B', 'KB', 'MB', 'GB'];
    let i = 0;
    while (n >= 1024 && i < 3) {
      n /= 1024;
      i++;
    }
    return `${n.toFixed(i ? 1 : 0)} ${u[i]}`;
  }

  function applySpeed() {
    if (!speedVal.trim()) return;
    let n = parseFloat(speedVal);
    if (isNaN(n) || n <= 0) return;
    const lower = speedVal.toLowerCase();
    if (lower.includes('mb')) n *= 1024 * 1024;
    else if (lower.includes('kb')) n *= 1024;
    else if (lower.includes('gb')) n *= 1024 * 1024 * 1024;
    onspeedlimit?.(task.id, Math.round(n));
    showSpeed = false;
    speedVal = '';
  }
</script>

<div class="row" class:done={task.state === 'done'} class:failed={task.state === 'failed'}>
  <div class="icon" class:spinning={task.state === 'downloading'}>
    {#if task.state === 'downloading'}⟳
    {:else if task.state === 'done'}✓
    {:else if task.state === 'paused'}⏸
    {:else if task.state === 'failed'}✕
    {:else if task.state === 'queued'}⏳
    {:else}…{/if}
  </div>

  <div class="body">
    <div class="top">
      <span class="name" title={task.name}>{task.name}</span>
      <div class="top-right">
        {#if task.category}
          <span class="cat-badge">{task.category}</span>
        {/if}
        <span
          class="state"
          class:accent={task.state === 'downloading'}
          class:err={task.state === 'failed'}
        >
          {t(stateKey)}
        </span>
      </div>
    </div>

    <div
      class="bar"
      role="progressbar"
      aria-label={t('task.speedLimit') + ' — ' + task.name}
      aria-valuemin="0"
      aria-valuemax="100"
      aria-valuenow={Math.round(pct * 100)}
    >
      <div
        class="fill"
        style:width={`${pct * 100}%`}
        class:indeterminate={task.total == null && task.state === 'downloading'}
        class:done-bar={task.state === 'done'}
        class:fail-bar={task.state === 'failed'}
      ></div>
    </div>

    <div class="meta">
      <span
        >{fmtBytes(task.downloaded)}{task.total != null ? ` / ${fmtBytes(task.total)}` : ''}</span
      >
      {#if task.state === 'downloading' && task.speed > 0}
        <span class="sep">·</span>
        <span class="speed">{fmtBytes(task.speed)}/s</span>
      {/if}
      {#if task.speed_limit > 0}
        <span class="sep">·</span>
        <span class="limit-badge">-limit {fmtBytes(task.speed_limit)}/s</span>
      {/if}
      {#if task.last_error}
        <span class="sep">·</span>
        <span class="err-msg" title={task.last_error}>{task.last_error}</span>
      {/if}
    </div>
  </div>

  <div class="actions">
    {#if task.state === 'downloading'}
      <button class="btn-action" title={t('toolbar.pause')} onclick={() => onpause?.(task.id)}
        >⏸</button
      >
    {:else if task.state === 'paused' || task.state === 'queued' || task.state === 'failed'}
      <button
        class="btn-action btn-gold"
        title={t('toolbar.resume')}
        onclick={() => onresume?.(task.id)}>▶</button
      >
    {/if}
    <button class="btn-action" title={t('task.speedLimit')} onclick={() => (showSpeed = !showSpeed)}
      >⚡</button
    >
    <button
      class="btn-action btn-danger"
      title={t('toolbar.remove')}
      onclick={() => onremove?.(task.id)}>✕</button
    >
  </div>
</div>

{#if showSpeed}
  <div class="speed-popup">
    <input
      type="text"
      placeholder={t('task.speedPlaceholder')}
      bind:value={speedVal}
      onkeydown={(e) => e.key === 'Enter' && applySpeed()}
    />
    <button class="btn-sm" onclick={applySpeed}>{t('task.apply')}</button>
    <button class="btn-sm btn-ghost" onclick={() => (showSpeed = false)}>{t('task.cancel')}</button>
  </div>
{/if}

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 16px;
    background: var(--bg-card);
    border: 1px solid var(--stroke);
    border-radius: var(--radius);
    transition: all 0.15s;
  }
  .row:hover {
    border-color: var(--stroke-strong);
    box-shadow: var(--shadow-sm);
  }
  .row.done {
    opacity: 0.7;
  }
  .row.failed {
    border-color: color-mix(in srgb, var(--danger) 25%, var(--stroke));
  }

  /* ═══════════ Icon ═══════════ */
  .icon {
    width: 38px;
    height: 38px;
    display: grid;
    place-items: center;
    border-radius: 10px;
    background: var(--bg-hover);
    font-size: 16px;
    flex-shrink: 0;
    color: var(--text-2);
    transition: all 0.2s;
  }
  .row:hover .icon {
    background: var(--accent-glow);
    color: var(--accent);
  }
  .row.done .icon {
    background: color-mix(in srgb, var(--success) 12%, var(--bg-hover));
    color: var(--success);
  }
  .row.failed .icon {
    background: color-mix(in srgb, var(--danger) 12%, var(--bg-hover));
    color: var(--danger);
  }
  .spinning {
    animation: spin 1.2s linear infinite;
    color: var(--accent);
    background: var(--accent-glow);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* ═══════════ Body ═══════════ */
  .body {
    flex: 1;
    min-width: 0;
  }
  .top {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
  }
  .name {
    font-weight: 600;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .top-right {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }
  .cat-badge {
    font-size: 10px;
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--accent-glow);
    color: var(--accent);
    font-weight: 600;
    border: 1px solid var(--stroke-strong);
  }
  .state {
    font-size: 11px;
    color: var(--text-3);
    font-weight: 500;
  }
  .state.accent {
    color: var(--accent);
  }
  .state.err {
    color: var(--danger);
  }

  /* ═══════════ Progress bar ═══════════ */
  .bar {
    height: 5px;
    border-radius: 4px;
    background: var(--bg-hover);
    margin: 8px 0 6px;
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 4px;
    background: linear-gradient(90deg, var(--accent-dim), var(--accent), var(--accent-strong));
    transition: width 0.35s ease;
    box-shadow: 0 0 6px var(--accent-glow);
  }
  .done-bar {
    background: var(--success);
    box-shadow: none;
  }
  .fail-bar {
    background: var(--danger);
    box-shadow: none;
  }
  .fill.indeterminate {
    width: 40% !important;
    animation: slide 1.4s ease-in-out infinite alternate;
  }
  @keyframes slide {
    from {
      margin-inline-start: 0;
    }
    to {
      margin-inline-start: 60%;
    }
  }

  /* ═══════════ Meta ═══════════ */
  .meta {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--text-3);
  }
  .speed {
    color: var(--accent);
    font-weight: 600;
  }
  .limit-badge {
    color: var(--purple);
    font-weight: 600;
    font-size: 10px;
  }
  .err-msg {
    color: var(--danger);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 220px;
    font-size: 11px;
  }

  /* ═══════════ Actions ═══════════ */
  .actions {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }
  .btn-action {
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border-radius: 8px;
    color: var(--text-3);
    font-size: 12px;
    transition: all 0.15s;
    border: 1px solid transparent;
  }
  .btn-action:hover {
    background: var(--accent-glow);
    color: var(--accent);
    border-color: var(--stroke-strong);
  }
  .btn-gold:hover {
    color: var(--accent-strong);
  }
  .btn-danger:hover {
    background: color-mix(in srgb, var(--danger) 12%, transparent);
    color: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 25%, transparent);
  }

  /* ═══════════ Speed popup ═══════════ */
  .speed-popup {
    display: flex;
    gap: 6px;
    padding: 0 16px 8px 68px;
    animation: fadeIn 0.15s;
  }
  .speed-popup input {
    flex: 1;
    font: inherit;
    font-size: 12px;
    color: var(--text);
    background: var(--bg-hover);
    border: 1px solid var(--stroke);
    border-radius: 8px;
    padding: 5px 10px;
    direction: ltr;
    transition: border-color 0.15s;
  }
  .speed-popup input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .btn-sm {
    padding: 5px 12px;
    border-radius: 8px;
    background: linear-gradient(135deg, var(--accent), var(--accent-strong));
    color: #1b1b1b;
    font-weight: 700;
    font-size: 11px;
    transition: all 0.15s;
  }
  .btn-sm:hover {
    filter: brightness(1.1);
  }
  .btn-ghost {
    background: transparent;
    border: 1px solid var(--stroke);
    color: var(--text-2);
  }
  .btn-ghost:hover {
    border-color: var(--stroke-strong);
    color: var(--text);
  }
  @keyframes fadeIn {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
</style>
