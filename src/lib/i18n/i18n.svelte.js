import en from './en.json';
import fa from './fa.json';

const dict = { en, fa };

function initial() {
  try {
    return localStorage.getItem('idin.lang') || 'fa';
  } catch {
    return 'fa';
  }
}

export const i18n = $state({ lang: initial() });

export function t(path) {
  const walk = (obj, keys) => keys.reduce((o, k) => o?.[k], obj);
  return walk(dict[i18n.lang], path.split('.')) ?? path;
}

export function setLang(lang) {
  i18n.lang = lang;
  try {
    localStorage.setItem('idin.lang', lang);
  } catch {
    /* private mode — language just won't persist */
  }
  document.documentElement.lang = lang;
  document.documentElement.dir = lang === 'fa' ? 'rtl' : 'ltr';
}
