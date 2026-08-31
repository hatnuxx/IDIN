<script>
  import { i18n, t, setLang } from '$lib/i18n/i18n.svelte.js';
  import TaskRow from '$lib/components/TaskRow.svelte';
  import { api } from '$lib/api';
  import BrowserSetup from '$lib/components/BrowserSetup.svelte';
  import SettingsPanel from '$lib/components/SettingsPanel.svelte';
  import HistoryView from '$lib/components/HistoryView.svelte';
  import { fly, fade } from 'svelte/transition';
  import { flip } from 'svelte/animate';
  import { fmtBytes } from '$lib/fmt.js';
  import { toast, getToasts } from '$lib/toast.svelte.js';

  let theme = $state('dark');
  let view = $state('downloads');
  let tasks = $state([]);
  let showAdd = $state(false);
  let showBrowserSetup = $state(false);
  let newUrl = $state('');
  let batchMode = $state(false);
  let errorMsg = $state('');

  // ── Live stats (from the engine) ──
  let stats = $state({ active: 0, active_speed: 0, total: 0, completed: 0 });
  const waitingCount = $derived(Math.max(0, stats.total - stats.active - stats.completed));

  // ── Download list search & status filter ──
  let searchQuery = $state('');
  let statusFilter = $state('all'); // all | downloading | paused | queued | done | failed

  const FILTERS = ['all', 'downloading', 'queued', 'paused', 'done', 'failed'];
  const filterCounts = $derived(
    Object.fromEntries(
      FILTERS.map((f) => [f, f === 'all' ? tasks.length : tasks.filter((tk) => tk.state === f).length]),
    ),
  );

  const filteredTasks = $derived(
    tasks.filter((tk) => {
      if (statusFilter !== 'all' && tk.state !== statusFilter) return false;
      if (!searchQuery) return true;
      return tk.name.toLowerCase().includes(searchQuery.toLowerCase());
    }),
  );

  const hasQuery = $derived(searchQuery.trim().length > 0);
  const listIsEmpty = $derived(tasks.length === 0);

  // ── Clipboard URL toast ──
  let clipUrl = $state(null); // URL offered from clipboard
  let toastTimeout = $state(null);
  api.onClipboardUrl?.((url) => {
    clipUrl = url;
    if (toastTimeout) clearTimeout(toastTimeout);
    toastTimeout = setTimeout(() => (clipUrl = null), 12000);
  });

  // ── Load tasks from the engine ──
  async function refresh() {
    try {
      const list = await api.list();
      tasks = list.map((tk) => ({
        id: tk.id,
        name: tk.destination.split(/[\\/]/).pop() || tk.url,
        state: tk.state,
        downloaded: tk.downloaded_bytes,
        total: tk.total_bytes,
        speed: tk.last_speed ?? 0,
        last_error: tk.last_error,
        progress: tk.total_bytes ? tk.downloaded_bytes / tk.total_bytes : 0,
        priority: tk.priority ?? 0,
        speed_limit: tk.speed_limit ?? 0,
        category: tk.category ?? null,
      }));
      api.getStats().then((s) => (stats = s)).catch(() => {});
    } catch {
      if (tasks.length === 0) loadDemo();
    }
  }

  function loadDemo() {
    tasks = [
      {
        id: 1,
        name: `demo-file.zip ${t('app.demoNote')}`,
        state: 'downloading',
        downloaded: 3.1e9,
        total: 5.2e9,
        speed: 4.6e6,
        progress: 0.6,
        category: 'Archives',
      },
    ];
  }

  refresh();
  // Clear the polling interval if the root component is ever unmounted.
  $effect(() => {
    const i = setInterval(refresh, 2000);
    return () => clearInterval(i);
  });

  // Live progress events
  api.onProgress?.((p) => {
    const tk = tasks.find((x) => x.id === p.task_id);
    if (!tk) return;
    tk.downloaded = p.downloaded_bytes;
    if (p.total_bytes != null) tk.total = p.total_bytes;
    tk.speed = p.speed_bps;
    tk.progress = p.total_bytes ? p.downloaded_bytes / p.total_bytes : tk.progress;
  });
  api.onState?.((id, state) => {
    const tk = tasks.find((x) => x.id === id);
    if (tk) {
      tk.state = state;
      if (state !== 'downloading') tk.speed = 0;
    }
    refresh();
  });

  // ── Auto-detect URLs from any pasted text ──
  function extractUrls(text) {
    if (!text) return [];
    const urlRegex = /https?:\/\/[^\s"'<>()[\]{}]+/gi;
    const urls = text.match(urlRegex) || [];
    // Trim trailing punctuation that isn't part of the URL
    return urls.map((u) => u.replace(/[),.;!?]+$/, ''));
  }

  // ── Duplicate download dialog (3.8) ──
  let showDup = $state(false);
  let dupInfo = $state(null); // DuplicateInfo from check_duplicate
  let pendingUrl = $state('');
  let pendingOptions = $state(null);

  async function resolveDuplicate(action) {
    const url = pendingUrl;
    showDup = false;
    try {
      await api.addDownload(url, '', 8, pendingOptions ?? undefined, action);
      refresh();
    } catch (e) {
      errorMsg = String(e);
      showAdd = true;
    }
    pendingUrl = '';
    pendingOptions = null;
    dupInfo = null;
  }

  function cancelDuplicate() {
    showDup = false;
    pendingUrl = '';
    pendingOptions = null;
    dupInfo = null;
  }

  // ── Add dialog ──
  // Advanced per-download options (headers / cookies / basic auth / proxy).
  let showAdvanced = $state(false);
  let advHeaders = $state(''); // one "Name: value" per line
  let advCookies = $state('');
  let advUser = $state('');
  let advPass = $state('');
  let advProxy = $state('');

  /** Parse "Name: value" lines into a header map. */
  function parseHeaders(text) {
    const map = /** @type {Record<string, string>} */ ({});
    for (const line of text.split('\n')) {
      const s = line.trim();
      if (!s) continue;
      const i = s.indexOf(':');
      if (i > 0) map[s.slice(0, i).trim()] = s.slice(i + 1).trim();
    }
    return map;
  }

  /** Collect advanced fields; null when everything is empty. */
  function collectOptions() {
    const headers = parseHeaders(advHeaders);
    const hasAny =
      Object.keys(headers).length > 0 ||
      advCookies.trim() ||
      advUser.trim() ||
      advPass.trim() ||
      advProxy.trim();
    if (!hasAny) return null;
    return {
      headers: Object.keys(headers).length ? headers : undefined,
      cookies: advCookies.trim() || undefined,
      username: advUser.trim() || undefined,
      password: advPass || undefined,
      proxy: advProxy.trim() || undefined,
    };
  }

  function resetAdvanced() {
    showAdvanced = false;
    advHeaders = '';
    advCookies = '';
    advUser = '';
    advPass = '';
    advProxy = '';
  }

  async function addDownload() {
    errorMsg = '';
    const raw = newUrl.trim();
    if (!raw) {
      errorMsg = t('app.noUrl');
      return;
    }

    try {
      const options = collectOptions();
      if (batchMode) {
        // In batch mode, auto-detect URLs from each line (may contain extra text)
        const detected = extractUrls(raw);
        if (detected.length === 0) {
          errorMsg = t('app.noValidUrl');
          return;
        }
        await api.addDownloads(detected, '', 8);
      } else {
        const urls = extractUrls(raw);
        if (urls.length === 0) {
          errorMsg = t('app.noValidUrl');
          return;
        }
        if (urls.length === 1) {
          // Ask the user what to do when the URL/file already exists.
          let info = null;
          try {
            info = await api.checkDuplicate(urls[0], '');
          } catch {
            /* check unavailable → fall back to engine auto-handling */
          }
          if (info?.duplicate) {
            pendingUrl = urls[0];
            pendingOptions = options;
            dupInfo = info;
            showDup = true;
            return;
          }
          await api.addDownload(urls[0], '', 8, options ?? undefined);
        } else {
          // Multiple links pasted into single-line field → switch to batch
          await api.addDownloads(urls, '', 8);
        }
      }
      newUrl = '';
      showAdd = false;
      batchMode = false;
      resetAdvanced();
      refresh();
      toast(t('app.addedToast'));
    } catch (e) {
      errorMsg = String(e);
      toast(t('app.addFailed'), 'err');
    }
  }

  // Detect content as user types/pastes: if multiple URLs appear, suggest batch
  function onUrlInput(e) {
    newUrl = /** @type {HTMLInputElement | HTMLTextAreaElement} */ (e.target).value;
    const urls = extractUrls(newUrl);
    if (urls.length > 1) batchMode = true; // auto-switch to multi-link mode
  }

  async function pause(id) {
    try {
      await api.pause(id);
      toast(t('toolbar.pause'));
      refresh();
    } catch {
      /* keep UI state; refresh picks it up */
    }
  }

  // Download a URL caught from the clipboard toast.
  async function downloadFromClip(url) {
    clipUrl = null;
    try {
      await api.addDownload(url, '', 8);
      toast(t('app.addedToast'));
      refresh();
    } catch (e) {
      errorMsg = String(e);
      showAdd = true;
    }
  }
  async function resume(id) {
    try {
      await api.resume(id);
      toast(t('toolbar.resume'));
      refresh();
    } catch {
      /* keep UI state; refresh picks it up */
    }
  }
  async function remove(id) {
    try {
      await api.remove(id);
      toast(t('app.removedToast'));
      refresh();
    } catch {
      /* keep UI state; refresh picks it up */
    }
  }
  async function speedLimit(id, bps) {
    try {
      await api.setTaskSpeedLimit(id, bps);
      refresh();
    } catch {
      /* keep UI state; refresh picks it up */
    }
  }

  function toggleTheme() {
    theme = theme === 'dark' ? 'light' : 'dark';
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem('idin.theme', theme);
    } catch {
      /* private mode — theme just won't persist */
    }
  }
</script>

<div class="shell">
  <aside class="side ornament-bg">
    <div class="brand">
      <span class="logo">🏛</span>
      <div class="brand-text">
        <span class="brand-name">IDIN</span>
        <span class="brand-sub">آیدین</span>
      </div>
    </div>

    <div class="ornament-divider"></div>

    <nav>
      {#each [['downloads', '⬇'], ['queue', '☰'], ['history', '🕘'], ['settings', '⚙']] as [v, icon] (v)}
        <button class="nav-item" class:active={view === v} onclick={() => (view = v)}>
          <span class="nav-icon" aria-hidden="true">{icon}</span>
          <span>{t(`nav.${v}`)}</span>
        </button>
      {/each}
    </nav>

    <div class="ornament-divider"></div>

    <div class="side-footer">
      <button class="ghost lang-btn" onclick={() => setLang(i18n.lang === 'fa' ? 'en' : 'fa')}>
        {i18n.lang === 'fa' ? 'EN' : 'فا'}
      </button>
      <button class="ghost" onclick={toggleTheme}>{theme === 'dark' ? '☀' : '🌙'}</button>
    </div>
  </aside>

  <main>
    <header class="topbar">
      <h1>{t(`nav.${view}`)}</h1>
      {#if view === 'downloads' || view === 'queue'}
        <button class="primary" onclick={() => (showAdd = true)}
          >＋ {t('toolbar.newDownload')}</button
        >
      {/if}
    </header>

    {#if view === 'downloads' || view === 'queue'}
      <!-- ══════════ Live stats bar ══════════ -->
      <div class="stats-row" transition:fade={{ duration: 120 }}>
        <div class="stat stat-accent">
          <span class="stat-val">{stats.active}</span>
          <span class="stat-label">{t('stats.active')}</span>
        </div>
        <div class="stat">
          <span class="stat-val">{fmtBytes(stats.active_speed)}/s</span>
          <span class="stat-label">{t('stats.speed')}</span>
        </div>
        <div class="stat">
          <span class="stat-val">{waitingCount}</span>
          <span class="stat-label">{t('stats.queued')}</span>
        </div>
        <div class="stat stat-done">
          <span class="stat-val">{stats.completed}</span>
          <span class="stat-label">{t('stats.done')}</span>
        </div>
      </div>

      <div class="list-toolbar">
        <div class="search-wrap">
          <span class="search-icon" aria-hidden="true">🔍</span>
          <input
            type="search"
            class="list-search"
            placeholder={t('app.searchPlaceholder')}
            bind:value={searchQuery}
          />
          {#if hasQuery}
            <button class="search-clear" title={t('dup.cancel')} onclick={() => (searchQuery = '')}
              >✕</button
            >
          {/if}
        </div>
        <div class="chips" role="tablist" aria-label="فیلتر وضعیت">
          {#each FILTERS as f (f)}
            <button
              class="chip"
              class:active={statusFilter === f}
              class:chip-dim={filterCounts[f] === 0 && f !== 'all'}
              onclick={() => (statusFilter = f)}
            >
              {f === 'all' ? t('stats.all') : t(`task.${f}`)}
              <span class="chip-count">{filterCounts[f]}</span>
            </button>
          {/each}
        </div>
      </div>
      <div class="list">
        {#each filteredTasks as tk (tk.id)}
          <div animate:flip={{ duration: 200 }}>
            <TaskRow
              task={tk}
              onpause={pause}
              onresume={resume}
              onremove={remove}
              onspeedlimit={speedLimit}
            />
          </div>
        {/each}
        {#if filteredTasks.length === 0}
          <div class="empty" in:fade={{ duration: 150 }}>
            {#if listIsEmpty}
              <div class="empty-icon">📥</div>
              <p class="empty-title">{t('empty.noDownloads')}</p>
              <p class="empty-hint">{t('empty.hint')}</p>
              <button class="primary empty-cta" onclick={() => (showAdd = true)}
                >{t('empty.cta')}</button
              >
            {:else}
              <div class="empty-icon">🔍</div>
              <p class="empty-title">{t('empty.noResults')}</p>
              {#if hasQuery}
                <button class="ghost" onclick={() => { searchQuery = ''; statusFilter = 'all'; }}
                  >{t('dup.cancel')}</button
                >
              {/if}
            {/if}
          </div>
        {/if}
      </div>
    {:else if view === 'history'}
      <HistoryView />
    {:else if view === 'settings'}
      <SettingsPanel />
    {:else}
      <div class="empty">
        <div class="empty-icon">📋</div>
        <p>{t('empty.noHistory') || 'تاریخچه خالی است'}</p>
      </div>
    {/if}
  </main>
</div>

{#if showBrowserSetup}
  <BrowserSetup open={showBrowserSetup} onclose={() => (showBrowserSetup = false)} />
{/if}

<!-- ═══════════ Global action toasts ═══════════ -->
{#if getToasts().length}
  <div class="toasts" aria-live="polite">
    {#each getToasts() as to (to.id)}
      <div
        class="app-toast"
        class:app-toast-err={to.kind === 'err'}
        transition:fly={{ y: 12, duration: 160 }}
      >
        {to.msg}
      </div>
    {/each}
  </div>
{/if}

{#if clipUrl}
  <div class="clip-toast" role="status" transition:fly={{ y: 20, duration: 200 }}>
    <div class="clip-toast-icon">📋</div>
    <div class="clip-toast-body">
      <div class="clip-toast-title">{t('app.clipTitle')}</div>
      <div class="clip-toast-url" title={clipUrl}>{clipUrl}</div>
    </div>
    <div class="clip-toast-actions">
      <button class="btn-toast-primary" onclick={() => downloadFromClip(clipUrl)}
        >⬇ {t('app.clipDownload')}</button
      >
      <button class="btn-toast-ghost" onclick={() => (clipUrl = null)}>✕</button>
    </div>
  </div>
{/if}

{#if showAdd}
  <div class="overlay" role="dialog" aria-modal="true" transition:fade={{ duration: 120 }}>
    <div class="dialog" transition:fly={{ y: 16, duration: 180 }}>
      <div class="dialog-header">
        <h3>{t('toolbar.newDownload')}</h3>
        <label class="batch-toggle">
          <input type="checkbox" bind:checked={batchMode} />
          <span>{t('app.multiLink')}</span>
        </label>
      </div>

      {#if batchMode}
        <textarea
          placeholder={t('app.batchPlaceholder')}
          bind:value={newUrl}
          oninput={onUrlInput}
          rows="5"
          class="batch-input"></textarea>
        <p class="hint">{t('app.batchHint')}</p>
      {:else}
        <input
          type="text"
          placeholder={t('app.urlPlaceholder')}
          bind:value={newUrl}
          oninput={onUrlInput}
          onkeydown={(e) => e.key === 'Enter' && addDownload()}
        />
      {/if}

      <button class="ghost adv-toggle" onclick={() => (showAdvanced = !showAdvanced)}>
        {showAdvanced ? '▾' : '▸'} {t('app.advanced')}
      </button>
      {#if showAdvanced}
        <div class="adv-fields">
          <textarea
            class="batch-input"
            rows="2"
            placeholder={t('app.headersPlaceholder')}
            bind:value={advHeaders}></textarea>
          <p class="hint">{t('app.headersHint')}</p>
          <input
            type="text"
            class="adv-input"
            placeholder={t('app.cookiesPlaceholder')}
            bind:value={advCookies}
          />
          <div class="adv-row">
            <input type="text" class="adv-input" placeholder={t('app.username')} bind:value={advUser} />
            <input type="password" class="adv-input" placeholder={t('app.password')} bind:value={advPass} />
          </div>
          <input
            type="text"
            class="adv-input"
            placeholder={t('app.proxyPlaceholder')}
            bind:value={advProxy}
          />
        </div>
      {/if}

      {#if errorMsg}<p class="error">{errorMsg}</p>{/if}
      <div class="dialog-actions">
        <button
          class="ghost"
          onclick={() => {
            showAdd = false;
            batchMode = false;
          }}>{t('toolbar.clear')}</button
        >
        <button class="primary" onclick={addDownload}>{t('toolbar.add')}</button>
      </div>
    </div>
  </div>
{/if}

<!-- ═══════════ Duplicate download dialog (3.8) ═══════════ -->
{#if showDup && dupInfo}
  <div class="overlay" role="dialog" aria-modal="true" transition:fade={{ duration: 120 }}>
    <div class="dialog" transition:fly={{ y: 16, duration: 180 }}>
      <div class="dialog-header">
        <h3>⚠ {t('dup.title')}</h3>
        <button class="btn-toast-ghost" onclick={cancelDuplicate}>✕</button>
      </div>
      <p class="dup-msg">
        {dupInfo.kind === 'url' ? t('dup.urlExists') : t('dup.fileExists')}
      </p>
      {#if dupInfo.path}
        <p class="dup-path">{dupInfo.path}</p>
      {/if}
      <div class="dialog-actions">
        <button class="ghost" onclick={cancelDuplicate}>{t('dup.cancel')}</button>
        <button class="ghost" onclick={() => resolveDuplicate('rename')}
          >{t('dup.rename')}</button
        >
        <button class="ghost" onclick={() => resolveDuplicate('overwrite')}
          >{t('dup.overwrite')}</button
        >
        {#if dupInfo.kind === 'url'}
          <button class="primary" onclick={() => resolveDuplicate('resume')}
            >{t('dup.resume')}</button
          >
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .shell {
    display: flex;
    height: 100vh;
  }

  /* ═══════════ Sidebar — carved stone ═══════════ */
  .side {
    width: 230px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    padding: 16px 12px;
    border-inline-end: 1px solid var(--stroke-strong);
    background: var(--bg-sidebar);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 10px 4px;
  }
  .logo {
    width: 38px;
    height: 38px;
    display: grid;
    place-items: center;
    border-radius: 10px;
    background: linear-gradient(135deg, var(--accent), var(--accent-strong));
    color: #1b1b1b;
    font-size: 18px;
    box-shadow: 0 2px 12px var(--accent-glow);
  }
  .brand-text {
    display: flex;
    flex-direction: column;
    line-height: 1.2;
  }
  .brand-name {
    font-weight: 800;
    font-size: 16px;
    letter-spacing: 1.5px;
    color: var(--accent);
  }
  .brand-sub {
    font-size: 11px;
    color: var(--text-3);
    letter-spacing: 0.5px;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .nav-item {
    text-align: start;
    padding: 10px 14px;
    border-radius: var(--radius);
    color: var(--text-2);
    font-size: 13px;
    transition: all 0.15s;
    position: relative;
  }
  .nav-item:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .nav-item.active {
    background: var(--accent-glow);
    color: var(--accent);
    font-weight: 700;
    border-inline-start: 3px solid var(--accent);
    padding-inline-start: 11px;
  }

  .side-footer {
    margin-top: auto;
    display: flex;
    gap: 6px;
    padding: 0 4px;
  }
  .ghost {
    padding: 6px 12px;
    border-radius: var(--radius);
    color: var(--text-2);
    border: 1px solid var(--stroke);
    transition: all 0.15s;
    font-size: 12px;
  }
  .ghost:hover {
    background: var(--accent-glow);
    border-color: var(--stroke-strong);
    color: var(--accent);
  }
  .lang-btn {
    font-weight: 600;
  }

  /* ═══════════ Main area ═══════════ */
  main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 28px 14px;
  }
  h1 {
    font-size: 20px;
    font-weight: 700;
    background: linear-gradient(135deg, var(--text), var(--accent));
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    background-clip: text;
  }

  .primary {
    padding: 9px 20px;
    border-radius: var(--radius);
    background: linear-gradient(135deg, var(--accent), var(--accent-strong));
    color: #1b1b1b;
    font-weight: 700;
    transition: all 0.15s;
    box-shadow: 0 2px 12px var(--accent-glow);
    font-size: 13px;
  }
  .primary:hover {
    filter: brightness(1.1);
    box-shadow: 0 4px 20px var(--accent-glow);
    transform: translateY(-1px);
  }

  .list-toolbar {
    display: flex;
    gap: 8px;
    padding: 10px 20px 0;
    align-items: center;
  }
  .list-search {
    flex: 1;
  }
  .list {
    flex: 1;
    overflow-y: auto;
    padding: 8px 28px 28px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  /* ═══════════ Empty state ═══════════ */
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--text-3);
    font-size: 14px;
  }
  .empty-icon {
    font-size: 48px;
    opacity: 0.3;
  }
  .empty-hint {
    font-size: 12px;
    opacity: 0.6;
  }

  /* ═══════════ Add dialog ═══════════ */
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(15, 18, 25, 0.65);
    backdrop-filter: blur(4px);
    display: grid;
    place-items: center;
    z-index: 50;
  }
  .dialog {
    width: min(480px, 90vw);
    background: var(--bg-card);
    border: 1px solid var(--stroke-strong);
    border-radius: var(--radius-lg);
    padding: 28px;
    box-shadow: var(--shadow);
    animation: pop 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  @keyframes pop {
    from {
      transform: scale(0.92);
      opacity: 0;
    }
  }
  .dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
  }
  .dialog-header h3 {
    color: var(--accent);
    font-size: 16px;
  }

  .batch-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-2);
    cursor: pointer;
  }
  .batch-toggle input[type='checkbox'] {
    accent-color: var(--accent);
  }

  input,
  textarea {
    font: inherit;
    color: var(--text);
    background: var(--bg-hover);
    border: 1px solid var(--stroke);
    border-radius: 8px;
    padding: 7px 12px;
    transition: border-color 0.15s;
  }
  input:focus,
  textarea:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-glow);
  }
  input {
    width: 100%;
    margin: 10px 0 4px;
  }
  .batch-input {
    width: 100%;
    margin: 10px 0 4px;
    min-height: 120px;
    resize: vertical;
    direction: ltr;
    text-align: start;
    font-size: 13px;
    line-height: 1.6;
  }
  .hint {
    font-size: 11px;
    color: var(--text-3);
    margin-top: 4px;
  }
  .dialog-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 18px;
  }
  .error {
    color: var(--danger);
    font-size: 12px;
    margin-top: 8px;
  }

  /* ═══════════ Duplicate download dialog ═══════════ */
  .dup-msg {
    font-size: 13px;
    color: var(--text-2);
    margin-top: 4px;
  }
  .dup-path {
    font-size: 12px;
    color: var(--text-3);
    direction: ltr;
    text-align: start;
    word-break: break-all;
    background: var(--bg-hover);
    border: 1px solid var(--stroke);
    border-radius: 8px;
    padding: 8px 10px;
    margin-top: 8px;
  }

  /* ═══════════ Advanced download options ═══════════ */
  .adv-toggle {
    margin-top: 10px;
    font-size: 12px;
  }
  .adv-fields {
    margin-top: 6px;
  }
  .adv-row {
    display: flex;
    gap: 8px;
  }
  .adv-row .adv-input {
    flex: 1;
  }
  .adv-input {
    margin: 8px 0 0;
  }

  /* ═══════════ Clipboard toast ═══════════ */
  .clip-toast {
    position: fixed;
    right: 20px;
    bottom: 20px;
    z-index: 60;
    display: flex;
    align-items: center;
    gap: 12px;
    max-width: 420px;
    background: var(--bg-card);
    border: 1px solid var(--accent);
    border-radius: var(--radius-lg);
    padding: 14px 16px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    animation: slideUp 0.25s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  @keyframes slideUp {
    from {
      transform: translateY(16px);
      opacity: 0;
    }
  }
  .clip-toast-icon {
    font-size: 22px;
    flex-shrink: 0;
  }
  .clip-toast-body {
    min-width: 0;
    flex: 1;
  }
  .clip-toast-title {
    font-size: 12px;
    font-weight: 700;
    color: var(--accent);
    margin-bottom: 2px;
  }
  .clip-toast-url {
    font-size: 12px;
    color: var(--text-2);
    direction: ltr;
    text-align: start;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .clip-toast-actions {
    display: flex;
    gap: 6px;
    align-items: center;
    flex-shrink: 0;
  }
  .btn-toast-primary {
    padding: 7px 14px;
    border-radius: 8px;
    background: linear-gradient(135deg, var(--accent), var(--accent-strong));
    color: #1b1b1b;
    font-weight: 700;
    font-size: 12px;
    white-space: nowrap;
    transition: all 0.15s;
  }
  .btn-toast-primary:hover {
    filter: brightness(1.1);
  }
  .btn-toast-ghost {
    width: 28px;
    height: 28px;
    display: grid;
    place-items: center;
    border-radius: 6px;
    color: var(--text-3);
    transition: all 0.15s;
  }
  .btn-toast-ghost:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .ghost {
    padding: 6px 12px;
    border-radius: var(--radius);
    color: var(--text-2);
    border: 1px solid var(--stroke);
    transition: all 0.15s;
    font-size: 12px;
  }
  .ghost:hover {
    background: var(--accent-glow);
    border-color: var(--stroke-strong);
    color: var(--accent);
  }

  /* ═══════════ Live stats bar ═══════════ */
  .stats-row {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 10px;
    margin-bottom: 14px;
  }
  .stat {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    background: var(--bg-card);
    border: 1px solid var(--stroke);
    border-radius: var(--radius);
    padding: 10px 8px 8px;
    transition: border-color 0.15s, transform 0.15s;
  }
  .stat:hover {
    border-color: var(--stroke-strong);
    transform: translateY(-1px);
  }
  .stat-val {
    font-size: 17px;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
    color: var(--text);
    direction: ltr;
  }
  .stat-label {
    font-size: 11px;
    color: var(--text-3);
  }
  .stat-accent .stat-val {
    color: var(--accent);
  }
  .stat-done .stat-val {
    color: var(--success);
  }

  /* ═══════════ Search box ═══════════ */
  .search-wrap {
    position: relative;
    flex: 1;
    min-width: 180px;
  }
  .search-icon {
    position: absolute;
    inset-inline-start: 12px;
    top: 50%;
    transform: translateY(-50%);
    font-size: 12px;
    opacity: 0.55;
    pointer-events: none;
  }
  .list-search {
    width: 100%;
    padding-inline-start: 34px;
  }
  .search-clear {
    position: absolute;
    inset-inline-end: 8px;
    top: 50%;
    transform: translateY(-50%);
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border-radius: 6px;
    font-size: 11px;
    color: var(--text-3);
  }
  .search-clear:hover {
    background: var(--bg-hover);
    color: var(--text);
  }

  /* ═══════════ Filter chips ═══════════ */
  .chips {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border-radius: 20px;
    border: 1px solid var(--stroke);
    background: var(--bg-card);
    color: var(--text-2);
    font-size: 12px;
    transition: all 0.15s;
  }
  .chip:hover {
    border-color: var(--stroke-strong);
    color: var(--text);
  }
  .chip.active {
    background: linear-gradient(135deg, var(--accent), var(--accent-strong));
    border-color: transparent;
    color: #1b1b1b;
    font-weight: 700;
  }
  .chip-dim {
    opacity: 0.55;
  }
  .chip-count {
    font-size: 10px;
    font-weight: 700;
    background: color-mix(in srgb, currentColor 15%, transparent);
    border-radius: 10px;
    padding: 0 7px;
    font-variant-numeric: tabular-nums;
  }
  .chip.active .chip-count {
    background: rgba(0, 0, 0, 0.18);
  }

  /* ═══════════ Sidebar nav icons ═══════════ */
  .nav-icon {
    font-size: 13px;
    opacity: 0.8;
  }
  .nav-item {
    gap: 9px;
  }

  /* ═══════════ Empty state ═══════════ */
  .empty-title {
    font-size: 15px;
    font-weight: 700;
    color: var(--text-2);
  }
  .empty-cta {
    margin-top: 14px;
  }

  /* ═══════════ Global toasts ═══════════ */
  .toasts {
    position: fixed;
    bottom: 18px;
    inset-inline-start: 50%;
    transform: translateX(-50%);
    z-index: 80;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    pointer-events: none;
  }
  .app-toast {
    background: var(--bg-card);
    color: var(--text);
    border: 1px solid var(--stroke-strong);
    border-inline-start: 3px solid var(--success);
    border-radius: 10px;
    padding: 9px 18px;
    font-size: 12.5px;
    box-shadow: var(--shadow);
    white-space: nowrap;
  }
  .app-toast-err {
    border-inline-start-color: var(--danger);
  }
</style>
