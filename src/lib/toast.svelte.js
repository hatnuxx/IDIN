/** Tiny global toast store — `toast('msg')` or `toast('msg', 'err')`. */
let toasts = $state([]);

let nextId = 1;

export function toast(msg, kind = 'ok', ms = 3000) {
  const id = nextId++;
  toasts.push({ id, msg, kind });
  setTimeout(() => {
    const i = toasts.findIndex((t) => t.id === id);
    if (i >= 0) toasts.splice(i, 1);
  }, ms);
}

export function getToasts() {
  return toasts;
}
