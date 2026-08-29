// IDIN content script — detect navigations to large downloadable files
// and offer them to IDIN (IDM-style intercept).
const DL_EXT = /\.(zip|rar|7z|tar|gz|iso|exe|msi|dmg|pkg|deb|rpm|apk|mp4|mkv|avi|mp3|flac|wav|pdf|epub|pdf|docx?|xlsx?|pptx?|psd|ai|torrent)$/i;

let intercepted = false;

document.addEventListener('click', (e) => {
  if (intercepted) return;
  const a = e.target.closest?.('a[href]');
  if (!a) return;
  const url = a.href;
  if (!DL_EXT.test(new URL(url, location.href).pathname)) return;

  // Notify background → IDIN; don't block the browser's own handling,
  // the user can cancel the browser download if they prefer IDIN.
  chrome.runtime.sendMessage({ type: 'idin-add', url });
}, true);

// Large-file interception via response headers is limited in MV3 content
// scripts; the context menu covers everything else.
