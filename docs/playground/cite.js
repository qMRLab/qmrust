// "Cite this work": the qMRLab paper, in whichever style the reader needs.
//
// The strings are pre-rendered into `data/citation.json` by
// `scripts/fetch_citation.sh`, which asks doi.org to format the DOI. Rendering
// them at build time rather than in the browser means citing costs no request,
// works offline, and keeps this app's only outbound traffic the dataset fetch
// a reader explicitly asks for.
import { $ } from "./dom.js";
import { paintIcons } from "./vendor/icons.js";

// Loaded once, on first open — a reader who never cites pays nothing.
let citation = null;

async function load() {
  if (citation) return citation;
  const r = await fetch("./data/citation.json");
  if (!r.ok) throw new Error(`${r.status}`);
  citation = await r.json();
  return citation;
}

// Every offered form: the CSL styles the registrar rendered, plus the two
// machine formats a reference manager wants.
function entries(c) {
  return [
    ...c.styles.map((s) => ({ id: s.id, label: s.label, text: s.text })),
    { id: "bibtex", label: "BibTeX", text: c.bibtex },
    { id: "ris", label: "RIS", text: c.ris },
  ];
}

function show(text) {
  $("cite-out").textContent = text;
  // A machine format is line-oriented and must keep its newlines; a prose
  // citation is one paragraph and should wrap to the dialog.
  $("cite-out").parentElement.classList.toggle("wrap", !text.includes("\n"));
}

async function openCite() {
  const modal = $("cite-modal");
  const select = $("cite-style");
  try {
    const c = await load();
    const forms = entries(c);
    if (select.options.length === 0) {
      for (const f of forms) {
        const o = document.createElement("option");
        o.value = f.id;
        o.textContent = f.label;
        select.append(o);
      }
      select.onchange = () => show(forms.find((f) => f.id === select.value).text);
    }
    show(forms.find((f) => f.id === select.value)?.text ?? forms[0].text);
    const link = $("cite-doi");
    link.href = c.url;
    link.textContent = c.doi;
  } catch (e) {
    // The payload is committed alongside the app, so a failure here means a
    // broken deploy rather than a network blip — say so instead of opening an
    // empty dialog.
    show(`Citation unavailable: could not read data/citation.json (${e.message})`);
  }
  modal.hidden = false;
  paintIcons(modal);
}

export function closeCite() {
  $("cite-modal").hidden = true;
}

export function wireCite() {
  $("cite-open").onclick = openCite;
  $("cite-close").onclick = closeCite;
  // The backdrop click and Escape are wired centrally in `app.js`, with every
  // other dialog, so all of them dismiss the same way.
  $("cite-copy").onclick = async () => {
    const label = $("cite-copy-label");
    try {
      await navigator.clipboard.writeText($("cite-out").textContent);
      label.textContent = "Copied";
    } catch {
      // Clipboard access can be refused (insecure origin, denied permission).
      // Selecting the text is the honest fallback: the reader copies it.
      const range = document.createRange();
      range.selectNodeContents($("cite-out"));
      const sel = window.getSelection();
      sel.removeAllRanges();
      sel.addRange(range);
      label.textContent = "Selected, now copy";
    }
    setTimeout(() => {
      label.textContent = "Copy";
    }, 1600);
  };
}
