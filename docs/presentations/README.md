# Project review presentation

`stormrfb-project-review.pptx` is a 16-slide, private project review of v0.1.1.
It covers project purpose, architecture, elapsed time, recorded tokens, difficult
implementation details, testing diagrams, first results, optimization and the
remaining real-guest integration gates. Charts, tables, diagrams and text are
editable. The red, black and white design contains no Red Hat logo.

## Evidence and accounting

Technical sources are [DESIGN.md](../DESIGN.md),
[VALIDATION.md](../VALIDATION.md) and [PERFORMANCE.md](../PERFORMANCE.md),
frozen at `ea3ced1`. Speaker notes carry measurement methods and source details.

Development elapsed time runs from the initial request at 2026-09-09
13:44:56.287 UTC to the final development token counter at 17:39:58.478 UTC:
3 hours 55 minutes. This includes builds, tool waits, approvals and discussion.
There were 23 commits after the spec-only `b7a6727` through `ea3ced1`.

The local session's cumulative counter at that cutoff reported 10,410,726
total tokens: 10,152,320 cached input, 190,140 noncached input and 68,266
output. Input totals 10,342,460, of which 98.2% was cached. The reported
12,425 reasoning output tokens are already part of output and are not added
again. These figures include replayed context, tool results and side questions.
They exclude the presentation work, do not represent unique text, and are not
a price calculation. Raw session logs and identifiers are not distributed.

## Rebuild

`build.mjs` uses the supplied `@oai/artifact-tool` JavaScript runtime and the
Presentations skill's finalizer. Run from the repository root with:

- `SKILL_DIR`: absolute path to the Presentations skill directory
- `RUNTIME_NODE_MODULES`: supplied runtime's absolute `node_modules` path
- `RUNTIME_PYTHON`: supplied runtime's Python executable
- `PRESENTATION_WORK_DIR`: a fresh writable build directory
- `REPO_ROOT`: optional repository root, defaults to the current directory

Place a copy of `build.mjs` in the build directory with a `node_modules` symlink
to the supplied modules, then execute it with the supplied Node executable.
The finalizer writes to `output/` and refuses to overwrite validation receipts.
Render the final PPTX with the skill's `render_presentation.mjs`, then inspect
every slide before replacing the committed deck. No Rust build is involved.
Rust validation continues to follow commit, push, and pull on the Linux host.

## Cover asset

`assets/cover.png` was generated using the built-in ImageGen tool in generation
mode. It is the only raster artwork. Prompt:

> Use case: stylized-concept. Create a premium abstract editorial background for
> a technical project presentation about framebuffer software and virtualization.
> 16:9 landscape. Predominantly near-black matte surface with a carefully composed
> cluster of deep red and vivid #ee0000 rectangular pixel-like tiles concentrated
> on the RIGHT third. Macro photographic / sculptural material quality, subtle
> lighting, restrained contrast, hard clean geometry, quiet technical aesthetic.
> LEFT two thirds almost plain black with generous negative space for editable
> white slide title placed later. No typography, no letters, no numbers, no logos,
> no Red Hat symbol, no hat shapes, no watermarks. Not a screenshot, not a diagram,
> not a slide mockup. Just the standalone background artwork.
