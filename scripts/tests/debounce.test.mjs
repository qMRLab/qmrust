// The recipe panel debounces the wasm surface refresh triggered by the YAML
// view's per-keystroke `oninput`, so it must run the first call in a burst
// immediately (a single edit, or a model load, is never delayed) and settle
// on the final call's arguments once the burst goes quiet — never drop the
// last keystroke, and never call into wasm once per character while typing.
import test from "node:test";
import assert from "node:assert/strict";
import { debounce } from "../../docs/playground/debounce.js";

test("a solitary call runs immediately", () => {
  const calls = [];
  const fn = debounce((v) => calls.push(v), 100);
  fn("a");
  assert.deepEqual(calls, ["a"]);
});

test("calls packed within `wait` collapse to one trailing call with the last arguments", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const calls = [];
  const fn = debounce((v) => calls.push(v), 100);
  fn("a");
  assert.deepEqual(calls, ["a"], "the leading call runs synchronously");
  fn("b");
  fn("c");
  assert.deepEqual(calls, ["a"], "no further call before the burst goes quiet");
  t.mock.timers.tick(100);
  assert.deepEqual(calls, ["a", "c"], "the trailing call carries the last call's arguments");
});

test("a call after the quiet period leads again", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const calls = [];
  const fn = debounce((v) => calls.push(v), 100);
  fn("a");
  t.mock.timers.tick(100);
  assert.deepEqual(calls, ["a", "a"], "the trailing call from the first burst has landed");
  fn("b");
  assert.deepEqual(calls, ["a", "a", "b"], "a new burst leads immediately again");
});
