# Human docs

This directory is the human-facing documentation site, built with
[MyST](https://mystmd.org): `npx mystmd start` to preview, `npx mystmd build --html`
to build. `myst.yml` lists an explicit `toc`, so the site only includes the
pages in this directory — it does not sweep in `docs/agents/` (agent-facing
design docs) or `docs/superpowers/` (gitignored planning scratch).
