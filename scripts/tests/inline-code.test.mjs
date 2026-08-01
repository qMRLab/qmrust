// A message that names a config key reads better with the term set apart. The
// message's author marks it; nothing guesses. Everything else must be escaped:
// a message can carry a filename or a value read from a dataset.
import test from "node:test";
import assert from "node:assert/strict";
import { inlineCodeHtml } from "../../docs/playground/inline-code.js";

test("a backticked term becomes code, the prose around it does not", () => {
  assert.equal(
    inlineCodeHtml("the recipe must not set `repetition_time`."),
    "the recipe must not set <code>repetition_time</code>.",
  );
});

test("several terms in one message are each marked", () => {
  assert.equal(
    inlineCodeHtml("supplies `flip_angles`, `repetition_time`"),
    "supplies <code>flip_angles</code>, <code>repetition_time</code>",
  );
});

test("markup in the message is escaped, inside and outside the code span", () => {
  // A message can quote a filename or a value from someone else's dataset.
  assert.equal(
    inlineCodeHtml('<img src=x onerror=alert(1)>'),
    "&lt;img src=x onerror=alert(1)&gt;",
  );
  assert.equal(inlineCodeHtml("`<b>`"), "<code>&lt;b&gt;</code>");
  assert.equal(inlineCodeHtml("a & b"), "a &amp; b");
});

test("an unmatched backtick stays a character and does not swallow the rest", () => {
  assert.equal(inlineCodeHtml("a ` b"), "a ` b");
  assert.equal(inlineCodeHtml("`unclosed"), "`unclosed");
});

test("a message with no backticks is unchanged apart from escaping", () => {
  assert.equal(inlineCodeHtml("Fit failed"), "Fit failed");
  assert.equal(inlineCodeHtml(""), "");
});
