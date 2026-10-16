# Agent Terminal Reading — product and UX contract

Status: **future design target, not implemented**. Updated 2026-10-16.
Applies to the experimental Agent Terminal / legacy `discreet_mode`.
The ordinary reader UI is not in scope.

## Outcome

Provide a terminal reading interface that resembles an authentic Coding
Agent conversation at a quick glance **and** supports comfortable sustained
reading of the original source. The two goals have equal priority.

"Low exposure" means lower visual salience of reading during casual
observation. It does not hide visible prose from somebody reading the screen;
folding is a convenience, not a security mechanism. Never promise privacy.

## Approved design direction

Replace "a book with error-log prefixes" and "a book with an Agent header"
with **Agent Conversation Reading**.

Four semantic regions:

1. **Session context** — quiet agent-like work context, no book title,
   author, chapter title, private path, or reader-specific headings.
2. **Tool/activity** — actual local reader operations, not invented builds
   or network calls. A Read event may represent loading a real chunk.
   Do not generate fake timestamps, severity tags or repeated filler.
3. **Assistant response** — a bounded passage of **verbatim** source text,
   with intact paragraphs, indentation and punctuation. One response
   unit per continuation is preferred over wall-to-wall full-page prose.
   No code block, per-line prefix, synthetic analysis, or rewritten text.
4. **Input/action** — commands or genuine key bindings to continue, go
   back, search, navigate and hide. A visibly editable `❯` prompt must
   work; until implemented use honest keyboard shortcuts, not fake input.

This is a **local visual metaphor**, not an LLM feature. No model call is
required. A displayed action must reflect a real Reader state transition,
and development/user docs must not present simulated UI as actual AI work.

### Illustration, not current program output

    > inspect context
    ● Read(local chunk)
      ⎿ Loaded source range 39–62

    ● Assistant
      [first original paragraph, unmodified]
      [second original paragraph, unmodified]

      ⎿ Output complete
    > continue_

Use a short tool line per content chunk, not a log for every source line.
Illustrative bracketed placeholders must never be rendered literally.

## Layout and reading comfort

- Primary visual acceptance viewport: a **~500px right-side terminal** in
  a 1400×900 split desktop. Also test various font sizes and terminal
  sizes (about 40, 60, and 80 columns). Layout engine uses **terminal
  cell columns**, not hard-coded pixel width.
- Experiment with **10–16 useful body rows per response** based on
  paragraph boundaries and viewport height; tune by actual reading,
  not by max-fill rules. It is a starting hypothesis, not a fixed cap.
- Preserve legible foreground contrast, CJK punctuation line-breaking,
  natural indentation and blank lines. Preserve exact original text.
- Keep muted chrome and visible text distinct; avoid book-style cards,
  oversized decoration and distracting animation. Show text immediately.
- A new chunk should appear as a natural Agent continuation, without
  reprinting the same header and pseudo-execution lines on every turn.
- Do not optimize low exposure by clipping, blurring or dimming prose.
  The intended trade-off is a plausible transcript and an immediate fold
  action, not unreadable text.

## State, navigation and privacy invariants

- `TextPosition { section_index, char_offset }` remains the only persisted
  reading anchor. Chunks, pages and transcript entries are derived views.
- `j` / `k` remain usable as next/back until a functional command parser
  provides equivalent behavior. If command input is implemented, the visible
  commands `continue`, `back`, `goto`, `find`, `status`, and `hide` must
  perform their advertised actions.
- Search, bookmarks, resize, mode switching, forward/back and F12 fold
  must preserve exact source position. Long paragraphs may split, but no
  characters can silently disappear or be consumed twice.
- Fold immediately removes prose, titles, queries and notifications from
  the TUI buffer; unfolding returns to the same anchor.
- Retain legacy `discreet_mode` / `privacy_mode` configuration aliases,
  unless a separate migration is explicitly approved.

## Explicit rejections

- INFO/WARN/ERROR logging, fake stack traces, or timestamps per line.
- A static four-line Agent header followed by an **entire book page**
  occupying the remaining terminal.
- A nonfunctional prompt that visually promises interactive commands.
- Fabricated subprocess work, an imaginary thinking trace or continuous
  bogus tool calls to camouflage reading.
- Per-line `assistant:` prefixes, markdown code fences around prose,
  book-like decorative borders, or font contrast so low it harms reading.
- Tests that treat maximum prose occupancy (e.g. 18/24 rows) as proof
  of authentic Agent interaction.

## Verification contract

**Functional**:

- Concatenating sequential non-overlap source ranges reconstructs the
  original paragraphs exactly, including long Chinese runs, English,
  indentation, empty lines and chapter boundaries.
- Go forward/back 30 times; resize, switch mode, fold/unfold, restart and
  resume without missing text or drifting anchors.
- Verify narrow/short viewports and book-end conditions. No panics.
- If input prompt is enabled, test actual editing, dispatch, cancellation
  and invalid commands; a static render assertion is insufficient.
- Test at least two consecutive chunks and the folded state, not only
  a single screenshot. Replace layout tests that force old assumptions.

**Manual**:

- Capture split-desktop screenshots for reading, continue, search/action,
  and folded views, at about 500px terminal width.
- Compare the visual hierarchy with a real coding-agent console.
  Ask whether the **first two seconds** feel like a developer session.
- Run a **20–30 minute Chinese reading trial**, noting comfort, readability,
  fatigue and awkward navigation. Record trade-offs, not claims of total
  invisibility. No vendor trademark or fake service integration is needed.

## Implementation status and sequence

**Baseline on 2026-10-15 feature branch:** the code implements a fixed
simulated header, a single `page.lines` region, static prompt and fold
screen. There is no interactive command parser or actual Agent message
history; existing tests assert a four-row header, 18-row body, two-row
footer at 80×24. This is an implementation fact, **not the design target**.

1. Produce the four screen-state mockups and document the trade-offs.
2. Implement a real source-position-bound response chunk and activity
   structure; preserve stable reading data and existing useful key bindings.
3. Only display command input as interactive when implemented.
4. Rewrite outdated tests; run Rust checks and manual screenshots.
5. After shipping, update `usage.md` to accurately describe what works.

Do not claim the design has shipped because this document is committed.
See [documentation index](../README.md) and
[historical experiments](../notes/阅读体验优化讨论.md).
