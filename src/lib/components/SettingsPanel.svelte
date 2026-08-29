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

  // ── Jalali (Solar Hijri) scheduler state ──
  const jalaliMonths = [
    'فروردین', 'اردیبهشت', 'خرداد', 'تیر', 'مرداد', 'شهریور',
    'مهر', 'آبان', 'آذر', 'دی', 'بهمن', 'اسفند',
  ];
  const jalaliMonthsEn = [
    'Farvardin', 'Ordibehesht', 'Khordad', 'Tir', 'Mordad', 'Shahrivar',
    'Mehr', 'Aban', 'Azar', 'Dey', 'Bahman', 'Esfand',
  ];
  let jalaliYear = $state(1405);
  let jalaliMonth = $state(1);
  let jalaliDay = $state(1);
  let jalaliPreview = $state('');
  let jalaliHour = $state(8);
  let jalaliMinute = $state(0);

  /** Convert a unix timestamp to a human-readable Jalali string. */
  function jalaliOf(ts) {
    // Synchronous conversion using the well-known jalaali algorithm
    // (mirrors the Rust engine module; kept in JS for instant display).
    const d = new Date(ts * 1000);
    return gregorianToJalaliStr(d.getFullYear(), d.getMonth() + 1, d.getDate()) +
      ` ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  }

  function div(a, b) { return Math.trunc(a / b); }
  function mod(a, b) { return a - b * Math.floor(a / b); }

  function jalCal(jy) {
    const breaks = [-61, 9, 38, 199, 426, 686, 756, 818, 1111, 1181, 1210, 1635, 2060, 2097, 2192, 2262, 2324, 2394, 2456, 3178];
    const gy = jy + 621;
    let leapJ = -14;
    let jp = breaks[0];
    let jump = 0;
    for (let i = 1; i < breaks.length; i++) {
      const jm = breaks[i];
      jump = jm - jp;
      if (jy < jm) break;
      leapJ += div(jump, 33) * 8 + div(mod(jump, 33), 4);
      jp = jm;
    }
    let n = jy - jp;
    leapJ += div(n, 33) * 8 + div(mod(n, 33) + 3, 4);
    if (mod(jump, 33) === 4 && jump - n === 4) leapJ += 1;
    const leapG = div(gy, 4) - div((div(gy, 100) + 1) * 3, 4) - 150;
    const march = 20 + leapJ - leapG;
    if (jump - n < 6) n = n - jump + div(jump + 4, 33) * 33;
    let leap = mod(mod(n + 1, 33) - 1, 4);
    if (leap === -1) leap = 4;
    return { leap, gy, march };
  }

  function g2d(gy, gm, gd) {
    let d = div((gy + div(gm - 8, 6) + 100100) * 1461, 4) + div(153 * mod(gm + 9, 12) + 2, 5) + gd - 34840408;
    d = d - div(div(gy + 100100 + div(gm - 8, 6), 100) * 3, 4) + 752;
    return d;
  }

  function d2g(jdn) {
    let j = 4 * jdn + 139361631;
    j = j + div(div(4 * jdn + 183187720, 146097) * 3, 4) * 4 - 3908;
    const i = div(mod(j, 1461), 4) * 5 + 308;
    const gd = div(mod(i, 153), 5) + 1;
    const gm = mod(div(i, 153), 12) + 1;
    const gy = div(j, 1461) - 100100 + div(8 - gm, 6);
    return { gy, gm, gd };
  }

  function jalaliMonthLength(jy, jm) {
    if (jm <= 6) return 31;
    if (jm <= 11) return 30;
    return jalCal(jy).leap === 0 ? 30 : 29;
  }

  function j2d(jy, jm, jd) {
    const r = jalCal(jy);
    return g2d(r.gy, 3, r.march) + (jm - 1) * 31 - div(jm, 7) * (jm - 7) + jd - 1;
  }

  function d2j(jdn) {
    const gy = d2g(jdn).gy;
    let jy = Math.min(gy - 621, 3177);
    const r = jalCal(jy);
    let k = jdn - g2d(r.gy, 3, r.march);
    if (k >= 0) {
      if (k <= 185) return { jy, jm: 1 + div(k, 31), jd: mod(k, 31) + 1 };
      k -= 186;
    } else {
      jy -= 1;
      k += 179;
      if (r.leap === 1) k += 1;
    }
    return { jy, jm: 7 + div(k, 30), jd: mod(k, 30) + 1 };
  }

  function gregorianToJalaliStr(gy, gm, gd) {
    const j = d2j(g2d(gy, gm, gd));
    return `${j.jd} ${jalaliMonths[j.jm - 1]} ${j.jy}`;
  }

  /** Apply the Jalali date fields → schedule timestamp. */
  async function setScheduleJalali() {
    const jy = Number(jalaliYear), jm = Number(jalaliMonth), jd = Number(jalaliDay);
    if (!jy || !jm || !jd || jd > jalaliMonthLength(jy, jm)) {
      jalaliPreview = 'تاریخ نامعتبر است.';
      return;
    }
    const g = d2g(j2d(jy, jm, jd));
    // Build a local datetime at the chosen hour/minute.
    const dt = new Date(g.gy, g.gm - 1, g.gd, Number(jalaliHour) || 0, Number(jalaliMinute) || 0);
    const ts = Math.floor(dt.getTime() / 1000);
    const pad = (n) => String(n).padStart(2, '0');
    scheduleInput = `${g.gy}-${pad(g.gm)}-${pad(g.gd)}T${pad(jalaliHour)}:${pad(jalaliMinute)}`;
    jalaliPreview = `${jd} ${jalaliMonths[jm - 1]} ${jy} ≈ ${g.gy}-${pad(g.gm)}-${pad(g.gd)}`;
    try {
      await api.setSchedule(ts);
      if (config) config.scheduled_start = ts;
      saveMsg = t('settings.scheduleSetMsg');
      setTimeout(() => (saveMsg = ''), 2000);
    } catch (e) {
      saveMsg = t('settings.error') + String(e);
    }
  }

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

      <!-- Jalali date entry: pick a Solar Hijri date, converts to the field above -->
      <div class="schedule-row jalali-row">
        <span class="hint">📅 تقویم جلالی:</span>
        <select bind:value={jalaliMonth} class="schedule-input jalali-month" aria-label="ماه جلالی">
          {#each jalaliMonths as m, i}
            <option value={i + 1}>{m}</option>
          {/each}
        </select>
        <input
          type="number"
          min="1"
          max="31"
          bind:value={jalaliDay}
          class="schedule-input jalali-day"
          aria-label="روز جلالی"
          placeholder="روز"
        />
        <input
          type="number"
          min="1300"
          max="1500"
          bind:value={jalaliYear}
          class="schedule-input jalali-year"
          aria-label="سال جلالی"
          placeholder="سال"
        />
        <button class="btn-sm" onclick={setScheduleJalali}>اعمال تاریخ جلالی</button>
      </div>
      {#if jalaliPreview}
        <p class="hint">≈ {jalaliPreview}</p>
      {/if}

      {#if config.scheduled_start}
        <p class="current-speed">
          ✅ {t('settings.scheduleCurrent')}: {new Date(
            config.scheduled_start * 1000,
          ).toLocaleString()}
          <span class="hint">({jalaliOf(config.scheduled_start)})</span>
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

    <!-- ══════════ Concurrency ══════════ -->
    <section class="card">
      <h2>⚙️ دانلود همزمان</h2>
      <p class="hint">حداکثر تعداد دانلودهای همزمان (۰ = بدون محدودیت).</p>
      <div class="post-action-row">
        <input
          type="number"
          min="0"
          max="32"
          value={config.max_concurrent ?? 0}
          onchange={(e) => {
            const v = Math.max(0, Number(/** @type {HTMLInputElement} */ (e.target).value) || 0);
            config.max_concurrent = v;
            api.setMaxConcurrent(v);
            saveMsg = t('settings.saved');
            setTimeout(() => (saveMsg = ''), 2000);
          }}
          style="max-width: 120px"
        />
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
