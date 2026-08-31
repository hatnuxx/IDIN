/** Shared number formatting helpers for the UI. */

/** Format bytes as a human-readable string (B/KB/MB/GB). */
export function fmtBytes(n) {
  if (n == null || isNaN(n)) return '—';
  const u = ['B', 'KB', 'MB', 'GB'];
  let i = 0;
  while (n >= 1024 && i < 3) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(i ? 1 : 0)} ${u[i]}`;
}

/** Format a duration in seconds as a compact ETA string. */
export function fmtEta(seconds) {
  if (!isFinite(seconds) || seconds <= 0) return '—';
  const s = Math.round(seconds);
  const m = Math.floor(s / 60);
  const h = Math.floor(m / 60);
  if (h > 0) return `${h}h ${m % 60}m`;
  if (m > 0) return `${m}m ${s % 60}s`;
  return `${s}s`;
}
