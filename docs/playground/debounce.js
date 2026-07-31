// A leading+trailing debounce: the first call in a burst runs immediately, so a
// single event — a model load, one keystroke — is never delayed. Calls packed
// within `wait` of one another collapse to at most one more run once the burst
// goes quiet, so the last, final state is always the one that lands.
//
// The timer functions are injectable so this stays testable without a real
// clock; callers outside a test always use the platform's `setTimeout`.
export function debounce(fn, wait, { setTimeout: schedule = setTimeout, clearTimeout: cancel = clearTimeout } = {}) {
  let timer = null;
  return (...args) => {
    if (timer === null) fn(...args);
    cancel(timer);
    timer = schedule(() => {
      timer = null;
      fn(...args);
    }, wait);
  };
}
