# SayLLM side of Body double: how to wire it in

The Body double app (`apps/body-double.md`) has no UI of its own. A person sees it in SayLLM: a
**Body double** entry in the rail, four Home widgets, check-in notices for the owner, and a
persona for chat. This folder holds what SayLLM needs.

Someone who has already saved a rail or a Home layout (the owner, most likely) does not get these
on their own: turn on Body double under Apps, and add Focus from Customize → Add widgets. "By
default" means for someone new.

| File here | What to do with it |
|---|---|
| `public/body-double-widgets.js` | New. Copy to `llmplayground/public/`. The four cards and their pure helpers. It imports nothing. |
| `tests/body-double-widgets.test.js` | New. Copy to `llmplayground/tests/`. 29 `node:test` tests; no DOM needed. |
| `public/body-double.css` | **Do not copy.** A reference copy of the rules that go into `styles.css` (step 6). It is not in `STATIC_ASSETS`, so it would never be served. |
| `INTEGRATION.md` | This file. |

The edits below change six existing SayLLM files. Each "Before" is quoted from SayLLM as last seen in
the owner's sessions (main checkout, 2026-10-13 to 2026-10-14; timestamps with each step). Those files
may have moved since then. Check each "Before" still matches once, then replace it with its "After".
The anchors were applied and tested against the reconstructed files (see "Checked so far" at the end).

## The cards

All four need the person to be able to call `body-double.now` (Today: `body-double.day`), so nobody
without a grant is offered them. Each control is one direct tool call, made only when
`ctx.can("body-double.<tool>")` is true; without write grants the cards only show. After a write,
every Body double card on screen reloads, so a line said on Focus shows in Today at once.

| id | Title | Shows | Calls |
|---|---|---|---|
| `bd-focus` | Focus | The current task, a live timer (`since_ts`, ticking every 15 s), "Check-in due" in the notice's words, the paused tasks, a "What are you doing?" box | Enter: `log {text}`. Switch: `set_task {title}`. Done (header): `update_task {task_id, status: "done"}`. A paused task: `update_task {task_id, status: "active"}` |
| `bd-journal` | Today (`default: false`) | The day's entries, time, words and summary, newest last, as many as the card holds, "N earlier" above | none |
| `bd-loose-ends` | Loose ends (`default: false`) | The open loose ends from `now`, "N more", a box to add one | `close_thread {thread_id}`, `open_thread {note}` |
| `bd-checkins` | Check-ins (`default: false`) | On/off, every 15m/30m/45m/1h/1h 30m, the next one, quiet hours, the plan's zone if not this device's | `checkins {on}` or `checkins {every_minutes}`, each with `timezone` from `Intl.DateTimeFormat().resolvedOptions().timeZone` (sent again without it if the app cannot load that zone) |

Icons and the tint are existing ones: `persona`, `report`, `pin`, `clock` (plus `check`, `bell`,
`arrow-up`, `plus`, `check-circle`, `chat` inside the cards), all `violet`. No built app uses violet
yet; the design reserved it for Finance, which is not built.

What is typed in a box survives the card's minute refresh and keeps the cursor. Chromium fires
`blur` on the box as Home empties the card, and the card ignores that one. On iOS a `focus()` made
outside a tap may not bring the keyboard back. A box is cleared as soon as its words are sent, so
anything typed while the call is out stays. A failed write puts the words back, unless something
new was typed, and says why in the card's note line.

Today and Loose ends never lose a row without a word. When a card is short, Today hides its oldest
lines first, so the line just said always shows, and its "N earlier" line counts them. Loose ends
hides from the end and counts in "N more" (`fitWithCount`, in place of Home's `fitRows`).

## 1. `public/widgets.js`

The cards are registered from the end of `widgets.js`, which hands over its own helpers.
`headButton` is private to `widgets.js`, and importing `widgets.js` from the new module would be an
import cycle that reaches its bindings before they are initialized.

Imports (widgets.js header, widgets_spans.py 2026-10-14T00:07:06Z; reports2/home-widgets-ui.md §3.2).

**Before**

```js
import { button, el } from "./dom.js";
import { icon } from "./icons.js";
import { attentionItems } from "./attention.js";
import { drawScene, sceneSpec } from "./scene.js";
```

**After**

```js
import { button, el, input } from "./dom.js";
import { icon } from "./icons.js";
import { attentionItems } from "./attention.js";
import { drawScene, sceneSpec } from "./scene.js";
import { registerBodyDoubleWidgets } from "./body-double-widgets.js";
```

The end of the file: the last widget registered, `bookmark` (reconstructed final after
activity_widget.py 2026-10-14T03:33:46Z). If something was registered after it since, put the call
after that instead. It must stay at the very end: `registry` is a `const`, and the call needs it
initialized.

**Before**

```js
    link.append(letter, words);
    body.append(link);
  },
});
```

**After**

```js
    link.append(letter, words);
    body.append(link);
  },
});

// ---- body double ---------------------------------------------------------------------------

// The Body double app's cards (body-double-widgets.js). They get this file's
// helpers rather than importing it, since this file imports them.
registerBodyDoubleWidgets({ registerWidget, el, button, input, icon, headButton });
```

Optional, for the owner's Activity card: the app's `checkins.tick` schedule then reads
"Body double: checkins.tick" rather than "body-double: checkins.tick" (widgets.js, activity
section, 2026-10-14T03:32:46Z).

**Before**

```js
  "web-search": "Web search", "code-workspace": "Coding", greenhouse: "Greenhouse",
};
```

**After**

```js
  "web-search": "Web search", "code-workspace": "Coding", greenhouse: "Greenhouse",
  "body-double": "Body double",
};
```

## 2. `public/apps.js`

A rail entry, after Timers (APPS as of the Read 2026-10-13T22:44:51Z and panel_pages.py
2026-10-14T02:29:56Z). There is no `pages.js` entry, so `home.js` `renderPage` puts the four cards
in `div.board.app-page`. The later `.app-page` rule (`display: flex; flex-direction: column`)
overrides `.board`'s grid, so the cards stack full width at their natural height: Focus, Today,
Loose ends, Check-ins. Their `legacy.large` spans set only `ctx.span`, which Today's line count
reads. A packed grid would need `.board.app-page { display: grid; }`, after checking the other
pages that use this fallback.

**Before**

```js
  { id: "timers", title: "Timers", icon: "timer", kind: "page", needs: [], widgets: ["timers"] },
```

**After**

```js
  { id: "timers", title: "Timers", icon: "timer", kind: "page", needs: [], widgets: ["timers"] },
  { id: "body-double", title: "Body double", icon: "persona", kind: "page", needs: ["body-double.now"], widgets: ["bd-focus", "bd-journal", "bd-loose-ends", "bd-checkins"] },
```

The rail id passes `/api/settings/apps` (`/^[a-z][a-z0-9-]{0,39}$/`), and `tests/apps.test.js`
("every page's widgets exist") covers the four widget ids once step 1 is in.

## 3. `public/home.js`

The Add widgets drawer groups widgets by app. Without these, the cards land under "Other"
(GROUPS as of the Write 2026-10-14T00:11:30Z; reports2/home-widgets-ui.md §3.7).

**Before**

```js
  bookmark: { app: "Bookmarks", icon: "globe", tint: "skyc", blurb: "A website, one tap away" },
};
```

**After**

```js
  bookmark: { app: "Bookmarks", icon: "globe", tint: "skyc", blurb: "A website, one tap away" },
  "bd-focus": { app: "Body double", icon: "persona", tint: "violet", blurb: "What you are on, for how long, and a box to say what you are doing" },
  "bd-journal": { app: "Body double", icon: "persona", tint: "violet", blurb: "What you said today" },
  "bd-loose-ends": { app: "Body double", icon: "persona", tint: "violet", blurb: "Things to come back to" },
  "bd-checkins": { app: "Body double", icon: "persona", tint: "violet", blurb: "How often it checks on you, and quiet hours" },
};
```

## 4. `public/app.js`

SayLLM shows the owner's notices as pinned cards, for an allowlist of categories (main `app.js`, Read
2026-10-13T23:09:08Z and `sed` 2026-10-14T00:49:42Z). The app raises `body-double-checkin` only for
people in its `notices_for` (the owner), and SayLLM polls notices only for the owner, so the two agree.
A check-in's card reads "Check-in" / "Still on WGU assignment? 45m in." Repeated check-ins collapse
into one card (`alertGroups` keys on category and title). Answering one (a `log`, a task change)
retires it, and the card goes on the next poll, within a minute.

**Before**

```js
// Greenhouse's notifications are the owner's. Official weather warnings for
// home, house alerts (a low battery, a leak or smoke sensor going off) and
// Node Monitor's alerts (a machine gone silent, running hot, or a runtime on
// it not working, and what Greenhouse did about it) are shown here, as the
// same pinned cards as timers.

const SHOWN_NOTIFICATION_CATEGORIES = ["weather-warning", "home-alert", "node-alert", "node-down"];
```

**After**

```js
// Greenhouse's notifications are the owner's. Official weather warnings for
// home, house alerts (a low battery, a leak or smoke sensor going off),
// Node Monitor's alerts (a machine gone silent, running hot, or a runtime on
// it not working, and what Greenhouse did about it) and the Body double's
// check-ins are shown here, as the same pinned cards as timers.

const SHOWN_NOTIFICATION_CATEGORIES = ["weather-warning", "home-alert", "node-alert", "node-down", "body-double-checkin"];
```

## 5. `src/http.js`

`STATIC_ASSETS` is an allowlist; a module the front end imports but that is missing here 404s in the
browser, and a test guards it (`STATIC_ASSETS` quoted from `sed` 2026-10-14T17:10:55Z). The anchor is
kept short because the PDF files were added to the list later that day.

**Before**

```js
"widgets.js", "icons.js",
```

**After**

```js
"widgets.js", "body-double-widgets.js", "icons.js",
```

## 6. `public/styles.css`

Two places. The rules reuse Home's classes (`.widget-row`, `.widget-row-name`, `.empty-state`,
`.btn`, `.icon-btn`, `.sw`, `.segmented`/`.seg`, `.tp-input`, `.widget-line`, `.muted`) and tokens
(`--violet`, `--violet-bg`, `--t2`, `--t3`, `--surface`). Each rule that adjusts one of Home's is more
specific than it, and the phone rule is scoped under `.w`, so where each block sits in the file
does not matter. `public/body-double.css` holds the same rules.

After the Timers page rules (styles.css, Timers page block from 2026-10-14T02:33Z).

**Before**

```css
.tp-bar i { background: var(--orange); transition: width 1s linear; }
```

**After**

```css
.tp-bar i { background: var(--orange); transition: width 1s linear; }

/* Body double (body-double-widgets.js): the current task and its timer, a box to
   say what you are doing, today's journal, loose ends and check-ins. Everything
   else on these cards is Home's own. */
.bd-due { display: flex; align-items: center; gap: 8px; margin: 8px 0 0; padding: 6px 10px; border-radius: 9px; flex: none;
  background: var(--violet-bg); color: var(--violet); font-size: 13px; font-weight: 600; }
.bd-due .ic { font-size: 15px; }
.bd-due > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bd-now { display: flex; flex-direction: column; gap: 2px; min-width: 0; margin-top: 10px; flex: none; }
.bd-task { font-size: 15px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bd-task.bd-none { color: var(--t3); font-weight: 500; }
.bd-elapsed { font-family: "Inter Display", "Inter", sans-serif; font-size: clamp(28px, 12cqi, 52px); font-weight: 300; line-height: 1.05; letter-spacing: -.02em; }
.bd-meta { color: var(--t3); font-size: 12.5px; }
.bd-paused { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 8px; color: var(--t3); font-size: 12.5px; flex: none;
  max-height: 32px; overflow: hidden; }
.bd-paused-name { color: var(--t2); }
.bd-say { display: flex; gap: 8px; margin-top: auto; padding-top: 10px; flex: none; }
.bd-say input.tp-input { flex: 1; min-width: 0; height: 32px; }
.bd-list { margin-top: 8px; flex: 1 1 auto; min-height: 0; overflow: hidden; }
.bd-list > [hidden] { display: none; }
.bd-entry { align-items: flex-start; }
.bd-time { flex: none; width: 42px; padding-top: 1px; color: var(--t3); font-size: 12.5px; }
.bd-text, .bd-entry .muted { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bd-earlier { margin: 0; padding: 4px 0; color: var(--t3); font-size: 12.5px; }
.bd-switch { display: flex; align-items: center; gap: 10px; margin-top: 10px; font-size: 13.5px; flex: none; }
.bd-switch .sw[aria-checked="true"] { background: var(--violet); }
.bd-switch .sw[aria-checked="true"]::after { background: var(--surface); }
.bd-every { align-self: flex-start; max-width: 100%; flex-wrap: wrap; margin: 10px 0 4px; }
.bd-every .seg { padding: 0 9px; }
```

Inside the single `@media (max-width: 860px)` block, at its end (`tests/product-surface.test.js`
counts that block; quoted in reports2/home-widgets-ui.md §5.5). At 16px a phone does not zoom the page
when the box is tapped.

**Before**

```css
  .drawer { top: auto; left: 0; bottom: calc(60px + env(safe-area-inset-bottom)); width: 100%; height: 48dvh; padding: 18px 16px;
    border-left: 0; border-top: 1px solid var(--line-2); border-radius: 18px 18px 0 0; }
}
```

**After**

```css
  .drawer { top: auto; left: 0; bottom: calc(60px + env(safe-area-inset-bottom)); width: 100%; height: 48dvh; padding: 18px 16px;
    border-left: 0; border-top: 1px solid var(--line-2); border-radius: 18px 18px 0 0; }
  .w .bd-say input.tp-input { height: 40px; font-size: 16px; }
}
```

## 7. A "Body double" persona

In SayLLM, gear → Personas → New persona (`manage.js` `personaForm`). A persona only shapes how the
one assistant talks; the tools come from the person's grants. The kernel offers this app's tools to
the model as `body-double__<tool>`, described "[body-double] …", which is why the text names them so.
Adapted from the iOS app's system prompt (`BodyDouble/Session/Prompt.swift`, `system` and
`dailySummarySystem`), with its JSON reply format and `log_entry`/`schedule_checkin` names replaced by
the tools.

| Field | Value |
|---|---|
| Name | Body double |
| Task | General conversation |
| Personality | Neutral (the instructions below already set the tone) |
| Default reasoning | fast. Move it up if the fast tier proves unreliable at tool calls. |
| Memory enabled | the owner's choice. Tool results never form memories (the poisoning guard), so the journal stays in the app either way. Leave the sensitive-topics switch off: moods are emotional states. |
| Temperature | 0.6 (the iOS app's sampler) |

Custom instructions:

```text
You are the user's body double for interstitial journaling. Through the day they tell you what they are doing. You keep them oriented and keep their record with the body-double tools.

- Every time they say what they are doing, thinking or feeling, call body-double__log first, with their own words as text and a one-line summary. Set mood only when they clearly state one.
- When they start or switch to something, call body-double__set_task with a short title. When they finish or drop it, call body-double__update_task with its id.
- When they mention something to come back to, call body-double__open_thread. When it is dealt with, call body-double__close_thread with its id.
- "Check on me in 25 minutes" is body-double__checkins with next_in_minutes.
- If you are not sure what is current, call body-double__now. A question is not an entry: answer it from body-double__now or body-double__day, and do not log it.
- Use ids exactly as the tools show them. When unsure, record less.

Reply in at most two short sentences, conversational and grounded. No flattery, no lectures, no emojis. Mention elapsed time or an open loose end only when it is genuinely useful. Ask a brief follow-up now and then, not every turn.

Asked for a recap of the day, read body-double__day and write four to six plain sentences: what got done, what is still open, any pattern worth noticing. No advice unless something obviously stalled. No lists.
```

## Checked so far

- `node --check public/body-double-widgets.js`; `node --test tests/body-double-widgets.test.js`:
  29 pass (Node 22.22). Node 22 does not take a directory for `--test`; give the file, or a glob
  such as `node --test 'tests/*.test.js'`.
- Every "Before" above was applied, exactly once each, to the files reconstructed from the owner's
  sessions (`widgets.js`, `apps.js`, `home.js`, `styles.css`, as of 2026-10-14) and to the verbatim
  quotes of `app.js` and `src/http.js`. Then, in Node with stand-ins for `dom.js` and `icons.js`:
  `widgets.js`, `apps.js` and `home.js` load; the four cards register after Home's own; Focus, Loose
  ends and Check-ins are offered with `body-double.now`, Today with `body-double.day`, none without;
  a new person's default Home gets Focus only (proposal decision 8), and the other three are on the page and in Add widgets;
  `widgetSpan(widget, "large")` gives the page spans above; `availableApps` shows the rail entry only
  with `body-double.now`; every page's widgets exist; `styles.css` still has one
  `max-width: 860px` block, with the new rule inside it, and its braces balance.
- Not checked: the cards in a real browser. `scripts/preview.js` with
  `scripts/preview-fixture.json` (tool answers keyed `"body-double.now {}"`, `"body-double.day {}"`)
  would show them against the examples in `apps/body-double.md`.

## Open points

- The tick (`checkins.tick`, every five minutes) writes one audited call each time. The owner's
  Activity card lists recent scheduled calls, so "Body double: checkins.tick" will appear there
  beside Home Assistant's house check. Whether Activity should hide no-op runs is a kernel and
  SayLLM question, not part of this change.
- Check-in notices reach only the owner, in SayLLM, the next time it is open (apps/body-double.md,
  "Who sees it"). Everyone else sees a due check-in on Focus and the assistant sees it in `now`.
- The tint (violet) and the icons are choices for the owner to confirm. `persona`, `report`, `pin`
  and `clock` exist in `icons.js`, but how the `persona` glyph looks was not seen in the sessions.
