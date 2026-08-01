// A leading+trailing debounce: the first call in a burst runs immediately, so a
// single event (a model load, one keystroke) is never delayed. Calls packed
// within `wait` of one another collapse to one more run once the burst goes
// quiet, so the last, final state is always the one that lands.
//
// A solitary call runs once and only once. Scheduling the trailing run
// unconditionally would re-run `fn` a whole `wait` later with nothing new to
// say, and for a caller that rebuilds the DOM that late repeat can land after
// the reader has moved on and take focus with it.
//
// The timer functions are injectable so this stays testable without a real
// clock; callers outside a test always use the platform's `setTimeout`.
export function debounce(fn, wait, { setTimeout: schedule = setTimeout, clearTimeout: cancel = clearTimeout } = {}) {
  let timer = null;
  // Whether a call arrived while a window was already open, which is the only
  // thing the trailing run exists to deliver.
  let packed = false;
  return (...args) => {
    if (timer === null) {
      fn(...args);
      packed = false;
    } else {
      packed = true;
      cancel(timer);
    }
    timer = schedule(() => {
      timer = null;
      if (packed) {
        packed = false;
        fn(...args);
      }
    }, wait);
  };
}
