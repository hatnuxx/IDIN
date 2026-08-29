<script>
  import { t, i18n, setLang } from '$lib/i18n/i18n.svelte.js';
  import { api } from '$lib/api';

  let config = $state(null);
  let loading = $state(true);
  let saving = $state(false);
  let saveMsg = $state('');

  // Editing state
  let editingCat = $state(null); // index of category being edited, or -1 for new
  let editName = $state('');
  let editFolder = $state('');
  let editExts = $state('');

  // ── Schedule & post-action state ──
  let scheduleInput = $state(''); // datetime-local value
  let postAction = $state('none');

  // Load config on mount
  async function loadConfig() {
    try {
      config = await api.getConfig();
    } catch {
      config = {
        download_dir: '',
        categories: [],
        global_speed_limit: 0,
        close_to_tray: true,
        scheduled_start: null,
        post_download_action: null,
      };
    }
    loading = false;
    initScheduleState(config);
    postAction = config.post_download_action || 'none';
  }
  loadConfig();

  // Initialize schedule field from a saved scheduled_start timestamp
  function initScheduleState(cfg) {
    if (cfg && cfg.scheduled_start) {
      const d = new Date(cfg.scheduled_start * 1000);
      const pad = (n) => String(n).padStart(2, '0');
      scheduleInput = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
    } else {
      scheduleInput = '';
    }
  }

  async function clearSchedule() {
    try {
      await api.setSchedule(null);
      if (config) config.scheduled_start = null;
      scheduleInput = '';
      saveMsg = t('settings.scheduleClearedMsg');
      setTimeout(() => (saveMsg = ''), 2000);
    } catch (e) {
      saveMsg = t('settings.error') + String(e);
    }
  }

  async function setSchedule() {
    if (!scheduleInput) {
      await clearSchedule();
      return;
    }
    const ts = new Date(scheduleInput).getTime() / 1000;
    if (isNaN(ts) || ts <= 0) return;
    try {
      await api.setSchedule(Math.floor(ts));
      if (config) config.scheduled_start = Math.floor(ts);
      saveMsg = t('settings.scheduleSetMsg');
      setTimeout(() => (saveMsg = ''), 2000);
    } catch (e) {
      saveMsg = t('settings.error') + String(e);
    }
  }

  function onPostActionChange(e) {
    postAction = /** @type {HTMLSelectElement} */ (e.target).value;
    const action = postAction === 'none' ? null : postAction;
    api.setPostAction(action);
    if (config) config.post_download_action = action;
    saveMsg = t('settings.saved');
    setTimeout(() => (saveMsg = ''), 2000);
  }

  async function saveConfig() {
    if (!config) return;
    saving = true;
    saveMsg = '';
    try {
      await api.setConfig(config);
      saveMsg = t('settings.saved');
      setTimeout(() => (saveMsg = ''), 2000);
    } catch (e) {
      saveMsg = t('settings.error') + String(e);
    }
    saving = false;
  }

  function formatBytes(bps) {
    if (!bps) return t('settings.unlimited');
    const u = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
    let n = bps,
      i = 0;
    while (n >= 1024 && i < 3) {
      n /= 1024;
      i++;
    }
    return `${n.toFixed(i ? 1 : 0)} ${u[i]}`;
  }

  // ── Category editing ──
  function startEditCategory(idx) {
    if (idx === -1) {
      editingCat = -1;
      editName = '';
      editFolder = '';
      editExts = '';
    } else {
      editingCat = idx;
      const cat = config.categories[idx];
      editName = cat.name;
      editFolder = cat.folder;
      editExts = cat.extensions.join(', ');
    }
  }

  function saveCategory() {
    const exts = editExts
      .split(',')
      .map((e) => e.trim())
      .filter((e) => e.startsWith('.') && e.length > 1);
    if (!editName || !editFolder || exts.length === 0) return;

    const cat = { name: editName, folder: editFolder, extensions: exts };
    if (editingCat === -1) {
      config.categories = [...config.categories, cat];
    } else {
      config.categories = config.categories.map((c, i) => (i === editingCat ? cat : c));
    }
    editingCat = null;
  }

  function deleteCategory(idx) {
    config.categories = config.categories.filter((_, i) => i !== idx);
    if (editingCat === idx) editingCat = null;
  }

  function moveCategory(idx, dir) {
    const cats = [...config.categories];
    const newIdx = idx + dir;
    if (newIdx < 0 || newIdx >= cats.length) return;
    [cats[idx], cats[newIdx]] = [cats[newIdx], cats[idx]];
    config.categories = cats;
  }

  // ── Speed limit ──
  let speedInput = $state('');
  function applySpeedLimit() {
    const parsed = parseSpeed(speedInput);
    config.global_speed_limit = parsed;
    api.setSpeedLimit(parsed);
  }

  function parseSpeed(str) {
    str = str.trim().toLowerCase();
    if (!str || str === '0' || str === 'نامحدود') return 0;
    const match = str.match(/^([\d.]+)\s*(kb|mb|gb)?\/?s?$/);
    if (!match) return 0;
    let n = parseFloat(match[1]);
    if (match[2] === 'kb') n *= 1024;
    else if (match[2] === 'mb') n *= 1024 * 1024;
    else if (match[2] === 'gb') n *= 1024 * 1024 * 1024;
    return Math.round(n);
  }
</script>

{#if loading}
  <div class="loading">⏳</div>
{:else if config}
  <div class="settings-scroll">
    <!-- ══════════ Download Directory ══════════ -->
    <section class="card">
      <h2>📁 {t('settings.downloadDir') || 'پوشه دانلود'}</h2>
      <div class="dir-row">
        <input
          type="text"
          value={config.download_dir}
          oninput={(e) => (config.download_dir = /** @type {HTMLInputElement} */ (e.target).value)}
          placeholder="C:\Users\...\Downloads\IDIN"
          class="dir-input"
        />
      </div>
      <p class="hint">فایل‌ها بر اساس دسته‌بندی در زیرپوشه‌های این مسیر ذخیره می‌شوند.</p>
    </section>

    <!-- ══════════ Speed Limit ══════════ -->
    <section class="card">
      <h2>⚡ {t('settings.speedLimit') || 'محدودیت سرعت'}</h2>
      <div class="speed-row">
        <input
          type="text"
          placeholder="نامحدود یا مثلاً 1mb/s"
          bind:value={speedInput}
          onkeydown={(e) => e.key === 'Enter' && applySpeedLimit()}
        />
        <button class="btn-sm" onclick={applySpeedLimit}>اعمال</button>
      </div>
      <p class="current-speed">
        فعلی: {formatBytes(config.global_speed_limit)}
      </p>
    </section>

    <!-- ══════════ Categories ══════════ -->
    <section class="card">
      <div class="section-header">
        <h2>📂 {t('settings.categories') || 'دسته‌بندی فایل‌ها'}</h2>
        <button class="btn-add" onclick={() => startEditCategory(-1)}>＋ افزودن</button>
      </div>
      <p class="hint">
        فایل‌ها بر اساس پسوند خودکار در زیرپوشه مناسب قرار می‌گیرند. ترتیب مهم است — اولین تطبیق
        برنده می‌شود.
      </p>

      {#if editingCat !== null}
        <div class="cat-editor">
          <input type="text" placeholder="نام (مثلاً: ویدیوها)" bind:value={editName} />
          <input type="text" placeholder="نام پوشه (مثلاً: Videos)" bind:value={editFolder} />
          <input
            type="text"
            placeholder="پسوند‌ها با کاما (مثلاً: .mp4, .mkv, .avi)"
            bind:value={editExts}
            class="ext-input"
          />
          <div class="editor-actions">
            <button class="btn-sm" onclick={saveCategory}>ذخیره</button>
            <button class="btn-sm btn-ghost" onclick={() => (editingCat = null)}>لغو</button>
          </div>
        </div>
      {/if}

      <div class="cat-list">
        {#each config.categories as cat, idx (cat.folder)}
          <div class="cat-row" class:editing={editingCat === idx}>
            <div class="cat-info">
              <span class="cat-name">{cat.name}</span>
              <span class="cat-folder">📁 {cat.folder}</span>
              <span class="cat-exts">{cat.extensions.join(' ')}</span>
            </div>
            <div class="cat-actions">
              <button class="btn-icon" onclick={() => moveCategory(idx, -1)} disabled={idx === 0}
                >↑</button
              >
              <button
                class="btn-icon"
                onclick={() => moveCategory(idx, 1)}
                disabled={idx === config.categories.length - 1}>↓</button
              >
              <button class="btn-icon" onclick={() => startEditCategory(idx)}>✎</button>
              <button class="btn-icon btn-danger" onclick={() => deleteCategory(idx)}>✕</button>
            </div>
          </div>
        {/each}
      </div>
    </section>

    <!-- ══════════ Schedule ══════════ -->
    <section class="card">
      <h2>⏰ {t('settings.schedule') || 'زمان‌بندی دانلود'}</h2>
      <p class="hint">{t('settings.scheduleHint') || 'زمان شروع دانلودها را تنظیم کنید.'}</p>

      <div class="schedule-row">
        <input type="datetime-local" bind:value={scheduleInput} class="schedule-input" />
        <button class="btn-sm" onclick={setSchedule}>
          {t('settings.scheduleSet') || 'ثبت'}
        </button>
        {#if config.scheduled_start}
          <button class="btn-sm btn-ghost" onclick={clearSchedule}>
            🗙 {t('settings.scheduleClear') || 'پاک‌کردن'}
          </button>
        {/if}
      </div>

      {#if config.scheduled_start}
        <p class="current-speed">
          ✅ {t('settings.scheduleCurrent')}: {new Date(
            config.scheduled_start * 1000,
          ).toLocaleString()}
        </p>
      {/if}
    </section>

    <!-- ══════════ Post-download action ══════════ -->
    <section class="card">
      <h2>🔌 {t('settings.postAction') || 'پس از پایان دانلودها'}</h2>
      <p class="hint">{t('settings.postActionHint') || ''}</p>
      <div class="post-action-row">
        <select value={config.post_download_action || 'none'} onchange={onPostActionChange}>
          <option value="none">{t('settings.postActionNone') || 'هیچ کاری نکن'}</option>
          <option value="shutdown">{t('settings.postActionShutdown') || 'خاموش کردن رایانه'}</option
          >
          <option value="sleep">{t('settings.postActionSleep') || 'خواب (Sleep)'}</option>
          <option value="hibernate">{t('settings.postActionHibernate') || 'هیبرنیت'}</option>
        </select>
      </div>
    </section>

    <!-- ══════════ Appearance ══════════ -->
    <section class="card">
      <h2>🎨 {t('settings.title') || 'تنظیمات ظاهری'}</h2>
      <div class="setting">
        <span>{t('settings.language')}</span>
        <select
          value={i18n.lang}
          onchange={(e) => setLang(/** @type {HTMLSelectElement} */ (e.target).value)}
        >
          <option value="fa">فارسی</option>
          <option value="en">English</option>
        </select>
      </div>
      <div class="setting">
        <span>{t('settings.theme')}</span>
        <select
          onchange={(e) => {
            const theme = /** @type {HTMLSelectElement} */ (e.target).value;
            document.documentElement.dataset.theme = theme;
            try {
              localStorage.setItem('idin.theme', theme);
            } catch {
              /* private mode */
            }
          }}
        >
          <option value="dark">{t('settings.dark')}</option>
          <option value="light">{t('settings.light')}</option>
        </select>
      </div>
      <div class="setting">
        <div class="setting-label">
          <span>{t('settings.closeToTray') || 'بسته شدن در سیستم‌تری'}</span>
          <span class="setting-hint">{t('settings.closeToTrayHint') || ''}</span>
        </div>
        <label class="toggle">
          <input
            type="checkbox"
            checked={config.close_to_tray}
            onchange={(e) => {
              config.close_to_tray = /** @type {HTMLInputElement} */ (e.target).checked;
            }}
          />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </section>

    <!-- ══════════ Save ══════════ -->
    <div class="save-bar">
      {#if saveMsg}
        <span class="save-msg" class:err={saveMsg.startsWith('✕')}>{saveMsg}</span>
      {/if}
      <button class="btn-save" onclick={saveConfig} disabled={saving}>
        {saving ? '⏳ ...' : '💾 ' + (t('settings.save') || 'ذخیره تنظیمات')}
      </button>
    </div>
  </div>
{/if}

<style>
  .settings-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 8px 28px 28px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    max-width: 640px;
  }

  .loading {
    flex: 1;
    display: grid;
    place-items: center;
    font-size: 28px;
  }

  .card {
    background: var(--bg-card);
    border: 1px solid var(--stroke-strong);
    border-radius: var(--radius-lg, 14px);
    padding: 20px;
    box-shadow: var(--shadow-sm);
  }
  .card h2 {
    font-size: 14px;
    font-weight: 700;
    color: var(--accent);
    margin-bottom: 12px;
  }

  .hint {
    font-size: 12px;
    color: var(--text-3);
    line-height: 1.8;
    margin-top: 6px;
  }

  /* ── Directory ── */
  .dir-row {
    display: flex;
    gap: 8px;
  }
  .dir-input {
    flex: 1;
    font: inherit;
    color: var(--text);
    background: var(--bg-hover);
    border: 1px solid var(--stroke);
    border-radius: 8px;
    padding: 8px 12px;
    direction: ltr;
    text-align: start;
    transition: border-color 0.15s;
  }
  .dir-input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-glow);
  }

  /* ── Speed ── */
  .speed-row {
    display: flex;
    gap: 8px;
  }
  .speed-row input {
    flex: 1;
    font: inherit;
    color: var(--text);
    background: var(--bg-hover);
    border: 1px solid var(--stroke);
    border-radius: 8px;
    padding: 8px 12px;
    transition: border-color 0.15s;
  }
  .speed-row input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-glow);
  }
  .current-speed {
    font-size: 12px;
    color: var(--accent);
    margin-top: 6px;
    font-weight: 600;
  }

  /* ── Categories ── */
  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .btn-add {
    padding: 5px 14px;
    border-radius: 8px;
    border: 1px solid var(--accent);
    color: var(--accent);
    font-size: 12px;
    font-weight: 600;
    transition: all 0.15s;
  }
  .btn-add:hover {
    background: var(--accent-glow);
  }

  .cat-editor {
    margin: 12px 0;
    padding: 14px;
    background: var(--bg-hover);
    border: 1px solid var(--stroke-strong);
    border-radius: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .cat-editor input {
    font: inherit;
    color: var(--text);
    background: var(--bg-card);
    border: 1px solid var(--stroke);
    border-radius: 8px;
    padding: 7px 12px;
    transition: border-color 0.15s;
  }
  .cat-editor input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .ext-input {
    direction: ltr;
    text-align: start;
    font-size: 12px;
  }
  .editor-actions {
    display: flex;
    gap: 6px;
    margin-top: 4px;
  }

  .cat-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .cat-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    background: var(--bg-hover);
    border: 1px solid var(--stroke);
    border-radius: 10px;
    transition: all 0.15s;
  }
  .cat-row:hover {
    border-color: var(--stroke-strong);
  }
  .cat-row.editing {
    border-color: var(--accent);
    background: var(--accent-glow);
  }
  .cat-info {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }
  .cat-name {
    font-weight: 600;
    font-size: 13px;
    white-space: nowrap;
  }
  .cat-folder {
    font-size: 12px;
    color: var(--text-2);
  }
  .cat-exts {
    font-size: 11px;
    color: var(--text-3);
    direction: ltr;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 180px;
  }
  .cat-actions {
    display: flex;
    gap: 2px;
    flex-shrink: 0;
  }

  /* ── Settings rows ── */
  .setting {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 0;
    border-bottom: 1px solid var(--stroke);
  }
  .setting:last-child {
    border-bottom: none;
  }
  .setting-label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .setting-hint {
    font-size: 11px;
    color: var(--text-3);
    line-height: 1.5;
  }

  /* ── Toggle switch ── */
  .toggle {
    position: relative;
    display: inline-block;
    width: 40px;
    height: 22px;
    flex-shrink: 0;
    cursor: pointer;
  }
  .toggle input {
    opacity: 0;
    width: 0;
    height: 0;
  }
  .toggle-slider {
    position: absolute;
    inset: 0;
    background: var(--bg-hover);
    border: 1px solid var(--stroke-strong);
    border-radius: 22px;
    transition: all 0.25s;
  }
  .toggle-slider::before {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    background: var(--text-3);
    border-radius: 50%;
    transition: all 0.25s;
  }
  .toggle input:checked + .toggle-slider {
    background: var(--accent);
    border-color: var(--accent);
  }
  .toggle input:checked + .toggle-slider::before {
    transform: translateX(18px);
    background: #1b1b1b;
  }
  select {
    font: inherit;
    color: var(--text);
    background: var(--bg-hover);
    border: 1px solid var(--stroke);
    border-radius: 8px;
    padding: 6px 12px;
  }

  /* ── Schedule & post-action ── */
  .schedule-row {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
    margin-top: 8px;
  }
  .schedule-input {
    font: inherit;
    color: var(--text);
    background: var(--bg-hover);
    border: 1px solid var(--stroke);
    border-radius: 8px;
    padding: 8px 12px;
    direction: ltr;
    transition: border-color 0.15s;
  }
  .schedule-input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-glow);
  }
  .post-action-row {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .post-action-row select {
    flex: 1;
  }

  /* ── Buttons ── */
  .btn-sm {
    padding: 7px 16px;
    border-radius: 8px;
    background: linear-gradient(135deg, var(--accent), var(--accent-strong));
    color: #1b1b1b;
    font-weight: 700;
    font-size: 12px;
    transition: all 0.15s;
  }
  .btn-sm:hover {
    filter: brightness(1.1);
  }
  .btn-sm:disabled {
    opacity: 0.5;
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
  .btn-icon {
    width: 28px;
    height: 28px;
    display: grid;
    place-items: center;
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-3);
    transition: all 0.15s;
  }
  .btn-icon:hover {
    background: var(--accent-glow);
    color: var(--accent);
  }
  .btn-icon:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }
  .btn-icon.btn-danger:hover {
    background: color-mix(in srgb, var(--danger) 12%, transparent);
    color: var(--danger);
  }

  /* ── Save bar ── */
  .save-bar {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
    padding: 4px 0;
  }
  .save-msg {
    font-size: 12px;
    color: var(--success);
    font-weight: 600;
    animation: fadeIn 0.3s;
  }
  .save-msg.err {
    color: var(--danger);
  }
  @keyframes fadeIn {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  .btn-save {
    padding: 10px 24px;
    border-radius: 10px;
    background: linear-gradient(135deg, var(--accent), var(--accent-strong));
    color: #1b1b1b;
    font-weight: 700;
    font-size: 13px;
    transition: all 0.15s;
    box-shadow: 0 2px 12px var(--accent-glow);
  }
  .btn-save:hover {
    filter: brightness(1.1);
    transform: translateY(-1px);
  }
  .btn-save:disabled {
    opacity: 0.5;
    transform: none;
  }
</style>
