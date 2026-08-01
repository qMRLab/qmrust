// Backticked spans in a message become `<code>`.
//
// A message that names a config key, a filename or a BIDS field reads better
// when the term is set apart from the prose around it. The writer of the
// message marks those terms with backticks, the same convention the recipes and
// docs use; nothing here guesses which words are identifiers, because a
// heuristic would eventually mark an ordinary word and leave a real key plain.
//
// Everything outside the backticks is escaped first. A message can carry a
// filename, a path, or a value read from a dataset, so it must never be treated
// as markup even though it usually comes from this app's own code.

function escapeHtml(text) {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/**
 * `text` with backticked spans wrapped in `<code>`, everything else escaped.
 * An unmatched backtick is left as a literal character rather than swallowing
 * the rest of the message.
 */
export function inlineCodeHtml(text) {
  return String(text)
    .split(/`([^`]+)`/)
    .map((part, i) => (i % 2 === 1 ? `<code>${escapeHtml(part)}</code>` : escapeHtml(part)))
    .join("");
}
