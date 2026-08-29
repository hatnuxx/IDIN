<script>
  // History view: persistent log of done/failed/canceled downloads,
  // with search + outcome filter.
  import { t } from '$lib/i18n/i18n.svelte.js';
  import { api } from '$lib/api';

  let entries = $state([]);
  let loading = $state(true);
  let search = $state('');
  let outcome = $state('all');

  const jalaliMonths = [
    'فروردین', 'اردیبهشت', 'خرداد', 'تیر', 'مرداد', 'شهریور',
    'مهر', 'آبان', 'آذر', 'دی', 'بهمن', 'اسفند',
  ];

  function div(a, b) { return Math.trunc(a / b); }
  function mod(a, b) { return a - b * Math.floor(a / b); }
  function jalCal(jy) {
    const breaks = [-61, 9, 38, 199, 426, 686, 756, 818, 1111, 1181, 1210, 1635, 2060, 2097, 2192, 2262, 2324, 2394, 2456, 3178];
    const gy = jy + 621;
    let leapJ = -14, jp = breaks[0], jump = 0;
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
  function d2j(jdn) {
    const gy = d2g(jdn).gy;
    let jy = Math.min(gy - 621, 3177);
    const r = jalCal(jy);
    let k = jdn - g2d(r.gy, 3, r.march);
    if (k >= 0) {
      if (k <= 185) return { jy, jm: 1 + div(k, 31), jd: mod(k, 31) + 1 };
      k -= 186;
    } else {
      jy -= 1; k += 179;
      if (r.leap === 1) k += 1;
    }
    return { jy, jm: 7 + div(k, 30), jd: mod(k, 30) + 1 };
  }

  /** Format a unix timestamp as a Jalali date string. */
  function formatJalali(ts) {
    const d = new Date(ts * 1000);
    const j = d2j(g2d(d.getFullYear(), d.getMonth() + 1, d.getDate()));
    const time = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
    return `${j.jd} ${jalaliMonths[j.jm - 1]} ${j.jy} — ${time}`;
  }

  function formatSize(bytes) {
    if (bytes == null) return '—';
    const u = ['B', 'KB', 'MB', 'GB', 'TB'];
    let n = bytes, i = 0;
    while (n >= 1024 && i < 4) { n /= 1024; i++; }
    return `${n.toFixed(i ? 1 : 0)} ${u[i]}`;
  }

  const outcomeIcon = { done: '✅', failed: '❌', canceled: '🚫' };
  const outcomeLabel = { done: 'موفق', failed: 'ناموفق', canceled: 'لغوشده' };

  const filtered = $derived(
    entries.filter((e) => {
      if (outcome !== 'all' && e.outcome !== outcome) return false;
      if (!search) return true;
      const q = search.toLowerCase();
      return e.filename.toLowerCase().includes(q) || e.url.toLowerCase().includes(q);
    }),
  );

  async function load() {
    try {
      entries = await api.getHistory();
    } catch {
      entries = [];
    }
    loading = false;
  }
  load();

  async function clear() {
    await api.clearHistory();
    entries = [];
  }
</script>

<div class="history-view">
  <div class="history-toolbar">
    <input
      class="history-search"
      type="search"
      placeholder="جستجو در تاریخچه (نام فایل یا آدرس)…"
      bind:value={search}
    />
    <select bind:value={outcome} class="history-filter">
      <option value="all">همه</option>
      <option value="done">موفق</option>
      <option value="failed">ناموفق</option>
      <option value="canceled">لغوشده</option>
    </select>
    <button class="btn-sm btn-ghost" onclick={clear}>🗑 پاک‌کردن تاریخچه</button>
  </div>

  {#if loading}
    <p class="hint">در حال بارگذاری…</p>
  {:else if filtered.length === 0}
    <div class="empty">
      <div class="empty-icon">📋</div>
      <p>{t('empty.noHistory') || 'تاریخچه خالی است'}</p>
    </div>
  {:else}
    <div class="history-list">
      {#each filtered as e (e.id + '-' + e.finished_at)}
        <div class="history-row card">
          <span class="history-icon">{outcomeIcon[e.outcome] || '•'}</span>
          <div class="history-main">
            <span class="history-name">{e.filename}</span>
            <span class="history-url hint">{e.url}</span>
            {#if e.last_error}
              <span class="history-error hint">⚠ {e.last_error}</span>
            {/if}
          </div>
          <div class="history-meta">
            <span>{outcomeLabel[e.outcome] || e.outcome}</span>
            <span class="hint">{formatSize(e.total_bytes)}</span>
            <span class="hint">{formatJalali(e.finished_at)}</span>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .history-view {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px 20px;
    overflow-y: auto;
  }
  .history-toolbar {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .history-search {
    flex: 1;
  }
  .history-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .history-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
  }
  .history-icon {
    font-size: 1.2rem;
  }
  .history-main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .history-name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .history-url,
  .history-error {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: ltr;
  }
  .history-meta {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
    font-size: 0.82rem;
    white-space: nowrap;
  }
</style>
