// IDIN extension background — right-click "Download with IDIN" + click capture.
const HOST_NAME = 'com.hatnux.idin';

let port = null;

function connectNative() {
  if (port) return port;
  try {
    port = chrome.runtime.connectNative(HOST_NAME);
    port.onDisconnect.addListener(() => {
      port = null;
    });
    port.onMessage.addListener((msg) => {
      console.log('[IDIN host]', msg);
    });
  } catch (e) {
    console.error('IDIN native host not available:', e);
  }
  return port;
}

function sendToIdin(url, referrer) {
  const p = connectNative();
  if (!p) return false;
  p.postMessage({ type: 'add', url, referrer });
  return true;
}

// ---- Context menu ----
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: 'idin-download',
    title: 'دانلود با IDIN / Download with IDIN',
    contexts: ['link'],
  });
});

chrome.contextMenus.onClicked.addListener((info) => {
  if (info.menuItemId === 'idin-download' && info.linkUrl) {
    sendToIdin(info.linkUrl, info.pageUrl);
  }
});

// ---- Click capture on large media / known file types (like IDM) ----
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg?.type === 'idin-add' && msg.url) {
    sendResponse({ ok: sendToIdin(msg.url, sender.tab?.url) });
  }
  return false;
});
