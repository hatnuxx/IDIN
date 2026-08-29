<script>
  import { i18n, t, setLang } from '$lib/i18n/i18n.svelte.js';
  import TaskRow from '$lib/components/TaskRow.svelte';
  import { api } from '$lib/api';
  import BrowserSetup from '$lib/components/BrowserSetup.svelte';
  import SettingsPanel from '$lib/components/SettingsPanel.svelte';
  import HistoryView from '$lib/components/HistoryView.svelte';

  let theme = $state('dark');
  let view = $state('downloads');
  let tasks = $state([]);
  let showAdd = $state(false);
  let showBrowserSetup = $state(false);
  let newUrl = $state('');
  let batchMode = $state(false);
  let errorMsg = $state('');

  // ── Download list search & status filter ──
  let searchQuery = $state('');
  let statusFilter = $state('all'); // all | downloading | paused | queued | done | failed

  const filteredTasks = $derived(
    tasks.filter((tk) => {
      if (statusFilter !== 'all' && tk.state !== statusFilter) return false;
      if (!searchQuery) return true;
      return tk.name.toLowerCase().includes(searchQuery.toLowerCase());
    }),
  );

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
    } catch (e) {
      errorMsg = String(e);
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
      refresh();
    } catch (e) {
      errorMsg = String(e);
      showAdd = true;
    }
  }
  async function resume(id) {
    try {
      await api.resume(id);
      refresh();
    } catch {
      /* keep UI state; refresh picks it up */
    }
  }
  async function remove(id) {
    try {
      await api.remove(id);
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
      {#each ['downloads', 'queue', 'history', 'settings'] as v (v)}
        <button class="nav-item" class:active={view === v} onclick={() => (view = v)}>
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
      <div class="list-toolbar">
        <input
          type="search"
          class="list-search"
          placeholder="🔍 جستجوی دانلودها…"
          bind:value={searchQuery}
        />
        <select bind:value={statusFilter} class="list-filter" aria-label="فیلتر وضعیت">
          <option value="all">همه</option>
          <option value="downloading">در حال دانلود</option>
          <option value="queued">در صف</option>
          <option value="paused">متوقف</option>
          <option value="done">تمام‌شده</option>
          <option value="failed">ناموفق</option>
        </select>
      </div>
      <div class="list">
        {#each filteredTasks as tk (tk.id)}
          <TaskRow
            task={tk}
            onpause={pause}
            onresume={resume}
            onremove={remove}
            onspeedlimit={speedLimit}
          />
        {/each}
        {#if filteredTasks.length === 0}
          <div class="empty">
            <div class="empty-icon">📥</div>
            <p>{t('empty.noDownloads') || 'دانلودی وجود ندارد'}</p>
            <p class="empty-hint">{t('empty.hint') || 'روی «دانلود جدید» کلیک کنید'}</p>
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

{#if clipUrl}
  <div class="clip-toast" role="status">
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
  <div class="overlay" role="dialog" aria-modal="true">
    <div class="dialog">
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
  <div class="overlay" role="dialog" aria-modal="true">
    <div class="dialog">
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
  .list-filter {
    max-width: 150px;
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
</style>
