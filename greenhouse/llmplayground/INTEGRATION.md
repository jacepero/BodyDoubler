# SayLLM side of Body double: how to wire it in

The Body double app (`apps/body-double.md`) has no UI of its own. A person sees it in SayLLM: a
**Body double** entry in the rail, four Home widgets, check-in notices for the owner, and a
persona for chat: a Body double persona task, the only chats that offer the app's tools. This
folder holds what SayLLM needs.

Someone who has already saved a rail or a Home layout (the owner, most likely) does not get these
on their own: turn on Body double under Apps, and add Focus from Customize → Add widgets. "By
default" means for someone new.

| File here | What to do with it |
|---|---|
| `public/body-double-widgets.js` | New. Copy to `llmplayground/public/`. The four cards and their pure helpers. It imports nothing. |
| `tests/body-double-widgets.test.js` | New. Copy to `llmplayground/tests/`. 35 `node:test` tests; no DOM needed. |
| `tests/body-double-chat.test.js` | New. Copy to `llmplayground/tests/`. 5 tests of the Body double task in chat (steps 7 to 11). It imports SayLLM's own modules, so it runs only in SayLLM. |
| `public/body-double.css` | **Do not copy.** A reference copy of the rules that go into `styles.css` (step 6). It is not in `STATIC_ASSETS`, so it would never be served. |
| `INTEGRATION.md` | This file. |

The edits below change ten existing SayLLM files and one existing test. Each "Before" is quoted from `jacepero/greenhouse`
main at `d176a23` (2026-10-14) and occurs there exactly once. If the files have moved since, check
each "Before" still matches once, then replace it with its "After". "Checked so far" at the end says
what was run with them applied.

## The cards

All four need the person to be able to call `body-double.now` (Today: `body-double.day`), so nobody
without a grant is offered them. Each control is one direct tool call, made only when
`ctx.can("body-double.<tool>")` is true; without write grants the cards only show. After a write,
every Body double card on screen reloads, so a line said on Focus shows in Today at once.
Today's Export is the one control that is not a tool call (below).

| id | Title | Shows | Calls |
|---|---|---|---|
| `bd-focus` | Focus | The current task, a live timer (`since_ts`, ticking every 15 s), a due check-in as a question about the current task ("Still on WGU assignment? 45m in."), the paused tasks, a "What are you doing?" box | Enter: `log {text}`. Switch: `set_task {title}`. Done (header): `update_task {task_id, status: "done"}`. A paused task: `update_task {task_id, status: "active"}` |
| `bd-journal` | Today (`default: false`) | The day's entries, the time and what was said (the summary, or else the words cut short, as `day` gives them), newest last, as many as the card holds, "N earlier" above (it also counts the entries `day` left out: it returns the newest that fit a chat turn) | Export (header): fetches `/apps/body-double/export?format=markdown` through the kernel's proxy and saves the file. No tool call |
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

Export works the way SayLLM's own conversation export does (`export-ui.js`). The module sends no
request of its own. It uses:
- **SayLLM's locked kernel fetch:** `kernelFetch`, from `app.js`, which `home.js` puts on each
  card's ctx. Once the session is known to be gone, nothing is sent. The path is
  `/apps/body-double/export?format=markdown`, reached through the kernel's proxy.
- **SayLLM's response check:** `assertProtectedResponse`, from `auth.js`, which `widgets.js` hands
  to the module. A 401, or a login page after a redirect, is an `AuthExpiredError`. That goes to
  `ctx.report`, which raises SayLLM's own sign-in notice, and nothing is saved.

A refusal (no `read`, the app down) is a note on the card. A good answer is saved through a link on
the card, under the name the app's `content-disposition` gives. The journal never passes through a
tool, so it stays out of the chat model's context and the kernel's tool audit. Without `kernelFetch`
and `assertProtectedResponse` the card shows no Export at all, rather than fetch on its own.

The button is a plain word ("Export", class `ghost`), not a `more` link: on a narrow card SayLLM
shows a `more` button as a bare "›".

Today and Loose ends never lose a row without a word. When a card is short, Today hides its oldest
lines first, so the line just said always shows, and its "N earlier" line counts them. Loose ends
hides from the end and counts in "N more" (`fitWithCount`, in place of Home's `fitRows`).

## 1. `public/widgets.js`

The cards are registered from the end of `widgets.js`, which hands over its own helpers.
`headButton` is private to `widgets.js`, and importing `widgets.js` from the new module would be an
import cycle that reaches its bindings before they are initialized.

Imports, at the top of the file. `assertProtectedResponse` is for Export (above).

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
import { assertProtectedResponse } from "./auth.js";
import { registerBodyDoubleWidgets } from "./body-double-widgets.js";
```

The end of the file, after the last widget registered (`bookmark`). If something was registered
after it since, put the call after that instead. It must stay at the very end: `registry` is a
`const`, and the call needs it initialized.

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
registerBodyDoubleWidgets({ registerWidget, el, button, input, icon, headButton, assertProtectedResponse });
```

Optional, for the owner's Activity card: the app's `checkins.tick` schedule then reads
"Body double: checkins.tick" rather than "body-double: checkins.tick" (`APP_NAMES`, in the activity
section).

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

A rail entry, after Timers. There is no `pages.js` entry, so `home.js` `renderPage` puts the four cards
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

Three places. First, the Add widgets drawer groups widgets by app (`GROUPS`). Without these, the
cards land under "Other".

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

Second and third, for Export: Home takes `app.js`'s locked `kernelFetch` and puts it on each card's
ctx, beside `kernelJson` and `appJson`. A card then has the same guarded, non-JSON fetch SayLLM's own
export uses. The card ctx is the one with `config` (`card()`); the page ctx of `renderAppPage`
does not need it.

**Before**

```js
 * callTool(qualified, args) → the tool's result; kernelJson(path, options);
 * appJson(path, options) for SayLLM's own data; loadLayout(form) → { widgets,
```

**After**

```js
 * callTool(qualified, args) → the tool's result; kernelJson(path, options);
 * kernelFetch(path, options) → the Response, for a card that downloads a file;
 * appJson(path, options) for SayLLM's own data; loadLayout(form) → { widgets,
```

**Before**

```js
  callTool, kernelJson, appJson, loadLayout, saveLayout, report,
```

**After**

```js
  callTool, kernelJson, kernelFetch, appJson, loadLayout, saveLayout, report,
```

**Before**

```js
      config,
      callTool: cachedTool,
      kernelJson,
```

**After**

```js
      config,
      callTool: cachedTool,
      kernelJson,
      kernelFetch,
```

## 4. `public/app.js`

Three places. First, SayLLM shows the owner's notices as pinned cards, for an allowlist of
categories. The app raises `body-double-checkin` only for
the owner (its `notices_for`), and SayLLM polls notices only for the owner, so the two agree.
A check-in's card reads "Check-in" / "Still on your current task? 45m in." and, with loose ends
open, "1 loose end open." It carries none of the person's words: no task title, no note.
The Focus card names the task. Repeated check-ins collapse
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

Second, Home gets the locked `kernelFetch` (step 3). It is the wrapped one, from
`lockedRequest(emitKernelFetch, interactionsLocked)`, so "exactly three functions call `fetch`, each
behind the one lock" stays true.

**Before**

```js
const home = createHome({
  callTool,
  kernelJson: kernelJsonRequest,
```

**After**

```js
const home = createHome({
  callTool,
  kernelJson: kernelJsonRequest,
  kernelFetch,
```

Third, Compare (the panel that sends one prompt to each reasoning choice). It has no persona, so
it leaves the Body double app out, as every chat but a Body double persona's does (step 8).
Without this, each leg is offered the app's tools, and a prompt like "starting on the dishes now"
could be logged once per leg.

**Before**

```js
import { meetsMinimum, personaSelection } from "./persona-options.js";
```

**After**

```js
import { BODY_DOUBLE_APP, meetsMinimum, personaSelection } from "./persona-options.js";
```

**Before**

```js
      body: JSON.stringify({ model: alias, messages: [{ role: "user", content: prompt }], max_tokens: 600 }),
```

**After**

```js
      // A comparison has no persona, so it leaves the Body double app out,
      // as every chat but a Body double persona's does.
      body: JSON.stringify({
        model: alias, messages: [{ role: "user", content: prompt }], max_tokens: 600,
        tool_scope: { exclude_apps: [BODY_DOUBLE_APP] },
      }),
```

## 5. `src/http.js`

`STATIC_ASSETS` is an allowlist; a module the front end imports but that is missing here 404s in the
browser, and a test guards it. The anchor is kept short; the list is one long line.

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
specific than it: `.widget-row.bd-entry` and `.widget-line.bd-meta` against Home's single-class
`.widget-row` and `.widget-line`, for example. The phone rule is scoped under `.w`. So where each
block sits in the file does not matter. `public/body-double.css` holds the same rules.

After the Timers page rules.

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
.bd-meta, .widget-line.bd-meta { color: var(--t3); font-size: 12.5px; }
.bd-paused { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 8px; color: var(--t3); font-size: 12.5px; flex: none;
  max-height: 32px; overflow: hidden; }
.bd-paused-name { color: var(--t2); }
.bd-say { display: flex; gap: 8px; margin-top: auto; padding-top: 10px; flex: none; }
.bd-say input.tp-input { flex: 1; min-width: 0; height: 32px; }
.bd-list { margin-top: 8px; flex: 1 1 auto; min-height: 0; overflow: hidden; }
.bd-list > [hidden] { display: none; }
.widget-row.bd-entry { align-items: flex-start; }
/* The "N earlier" line comes first in the list; hidden, the first row still gets no divider. */
.bd-earlier[hidden] + .widget-row { border-top: 0; }
.bd-time { flex: none; width: 42px; padding-top: 1px; color: var(--t3); font-size: 12.5px; }
.bd-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bd-earlier { margin: 0; padding: 4px 0; color: var(--t3); font-size: 12.5px; }
.bd-switch { display: flex; align-items: center; gap: 10px; margin-top: 10px; font-size: 13.5px; flex: none; }
.bd-switch .sw[aria-checked="true"] { background: var(--violet); }
.bd-switch .sw[aria-checked="true"]::after { background: var(--surface); }
.bd-every { align-self: flex-start; max-width: 100%; flex-wrap: wrap; margin: 10px 0 4px; }
.bd-every .seg { padding: 0 9px; }
```

Inside the single `@media (max-width: 860px)` block, at its end (`tests/product-surface.test.js`
counts that block). At 16px a phone does not zoom the page
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

## 7. `public/persona-options.js`

The Body double persona is a persona **task**, "Body double", like "Software engineering" or
"Writing and editing". Its instruction is the persona text, adapted from the iOS app's system prompt
(`BodyDouble/Session/Prompt.swift`). Choosing the task is what makes a persona a body double:
- in that persona's chats, the kernel offers the Body double app's tools, and the turn reserves room
  for them;
- every other chat leaves the app out, and pays nothing for it.

The constants sit here because both the browser (the persona form lists `TASKS`) and the server
(`chat-lifecycle.js`) read this file.

**Before**

```js
export const TASKS = [
```

**After**

```js
// The Body double app (apps/body-double.md). Its tools are offered only in a
// chat whose persona has the Body double task, and those turns reserve room
// for the tools' schemas: about 690 tokens in the kernel's wire form,
// GREENHOUSE_BODY_DOUBLE_OVERHEAD_TOKENS to change. Every other chat leaves
// the app out (the kernel's tool scope) and pays nothing for it.
export const BODY_DOUBLE_APP = "body-double";
export const BODY_DOUBLE_TASK = "body-double";
export const BODY_DOUBLE_OVERHEAD_TOKENS = 720;

export const TASKS = [
```

**Before**

```js
    suggested: { preferredReasoning: "fast", temperature: 1 },
  },
];
```

**After**

```js
    suggested: { preferredReasoning: "fast", temperature: 1 },
  },
  {
    id: BODY_DOUBLE_TASK,
    label: "Body double",
    // Adapted from the iOS Body Double's system prompt.
    instruction: [
      "You are the user's body double for interstitial journaling. Through the day they tell you what they are doing. You keep them oriented and keep their record with the body-double tools.",
      "- When they start or switch to something, first call body-double__set_task with a short title. When they finish or drop it, call body-double__update_task with its id.",
      "- Every time they say what they are doing or thinking, call body-double__log, with their own words as text and a one-line summary.",
      "- When they mention something to come back to, call body-double__open_thread. When it is dealt with, call body-double__close_thread with its id.",
      "- \"Check on me in 25 minutes\" is body-double__checkins with next_in_minutes.",
      "- If you are not sure what is current, call body-double__now. A question is not an entry: answer it from body-double__now or body-double__day, and do not log it.",
      "- Use ids exactly as the tools show them. When unsure, record less.",
      "Reply in at most two short sentences, conversational and grounded. No flattery, no lectures, no emojis. Mention elapsed time or an open loose end only when it is genuinely useful. Ask a brief follow-up now and then, not every turn.",
      "Asked for a recap of the day, read body-double__day and write four to six plain sentences: what got done, what is still open, any pattern worth noticing. No advice unless something obviously stalled. No lists.",
    ].join("\n"),
    suggested: { preferredReasoning: "fast", temperature: 0.6 },
  },
];
```

## 8. `src/chat-lifecycle.js`

Four places:
- the import;
- the history budget, which reserves the Body double overhead only in that task's turns;
- the turn's tool scope, which leaves the app out of every other chat;
- the history trim, which uses that same budget. It computed its own before, from the same inputs,
  and would have left the reserve out of the history actually sent.

The scope uses the kernel's `exclude_apps`, as web search's switch already does. Excluding an app
the person has no grant for, or that is not registered, changes nothing.

**Before**

```js
import { composePersonaPrompt, meetsMinimum } from "../public/persona-options.js";
```

**After**

```js
import {
  BODY_DOUBLE_APP, BODY_DOUBLE_OVERHEAD_TOKENS, BODY_DOUBLE_TASK, composePersonaPrompt, meetsMinimum,
} from "../public/persona-options.js";
```

**Before**

```js
  const historyBudget = effectiveHistoryBudget({
    ...inferenceBudget,
    requestedLimit: persona.contextLimit || defaultContextLimit,
  });
```

**After**

```js
  // The Body double app's tools are offered only under a persona with its
  // task, and only those turns reserve room for their schemas.
  const bodyDouble = persona.task === BODY_DOUBLE_TASK;
  const historyBudget = effectiveHistoryBudget({
    ...inferenceBudget,
    reservedForOverhead: inferenceBudget.reservedForOverhead
      + (bodyDouble ? inferenceBudget.bodyDoubleOverhead ?? BODY_DOUBLE_OVERHEAD_TOKENS : 0),
    requestedLimit: persona.contextLimit || defaultContextLimit,
  });
```

**Before**

```js
  const webSearchOff = body?.webSearch === false;
  const inference = {
```

**After**

```js
  const webSearchOff = body?.webSearch === false;
  // Apps left out of this turn: web search when switched off, and the Body
  // double app outside a Body double persona's chats.
  const excludedApps = [...(webSearchOff ? [WEB_SEARCH_APP] : []), ...(bodyDouble ? [] : [BODY_DOUBLE_APP])];
  const inference = {
```

**Before**

```js
    ...(webSearchOff ? { tool_scope: { exclude_apps: [WEB_SEARCH_APP] } } : {}),
```

**After**

```js
    ...(excludedApps.length ? { tool_scope: { exclude_apps: excludedApps } } : {}),
```

**Before**

```js
  const trim = trimToBudget(history, {
    system,
    limit: effectiveHistoryBudget({
      ...inferenceBudget,
      requestedLimit: persona.contextLimit || defaultContextLimit,
    }),
  });
```

**After**

```js
  // The same budget as the early guard, the Body double reserve included.
  const trim = trimToBudget(history, { system, limit: historyBudget });
```

## 9. `src/config.js`

The reserve is configurable, `GREENHOUSE_BODY_DOUBLE_OVERHEAD_TOKENS`. The default, 720, is the size
of the app's eight people-facing tool schemas in the kernel's wire form (2,761 bytes, about 690
tokens by chars/4), rounded up. Startup refuses a value that would leave a Body double chat under
512 tokens of history, the same floor the plain budget has.

**Before**

```js
import { DEFAULT_ESTIMATOR_SAFETY_FACTOR } from "./trim.js";
```

**After**

```js
import { DEFAULT_ESTIMATOR_SAFETY_FACTOR } from "./trim.js";
import { BODY_DOUBLE_OVERHEAD_TOKENS } from "../public/persona-options.js";
```

**Before**

```js
  // The estimator's error grows with input size, so the budget is discounted by
```

**After**

```js
  // Added to the overhead in a Body double persona's turns, the only turns
  // that offer the Body double app's tools (persona-options.js).
  const bodyDoubleOverheadTokens = Number(
    envString("GREENHOUSE_BODY_DOUBLE_OVERHEAD_TOKENS", String(BODY_DOUBLE_OVERHEAD_TOKENS)),
  );
  if (!Number.isInteger(bodyDoubleOverheadTokens) || bodyDoubleOverheadTokens < 0) {
    throw new Error("GREENHOUSE_BODY_DOUBLE_OVERHEAD_TOKENS must be a non-negative integer");
  }
  // The estimator's error grows with input size, so the budget is discounted by
```

**Before**

```js
  if (estimatedPromptBudget < 512) {
    throw new Error(
      "the configured context and reservations leave under 512 estimated tokens for prompt history",
    );
  }
```

**After**

```js
  if (estimatedPromptBudget < 512) {
    throw new Error(
      "the configured context and reservations leave under 512 estimated tokens for prompt history",
    );
  }
  if (Math.floor((availablePromptSpace - bodyDoubleOverheadTokens) * estimatorSafetyFactor) < 512) {
    throw new Error(
      "with the Body double reserve, the configured context leaves under 512 estimated tokens for prompt history",
    );
  }
```

**Before**

```js
    reservedOverheadTokens,
    estimatorSafetyFactor,
```

**After**

```js
    reservedOverheadTokens,
    bodyDoubleOverheadTokens,
    estimatorSafetyFactor,
```

## 10. `src/server.js`

**Before**

```js
      reservedForOverhead: config.reservedOverheadTokens,
```

**After**

```js
      reservedForOverhead: config.reservedOverheadTokens,
      bodyDoubleOverhead: config.bodyDoubleOverheadTokens,
```

## 11. `tests/web-search.test.js`

Web search's test pinned `tool_scope` to nothing when web search is on. With the Body double app
left out of every chat but a Body double persona's, the scope now names `body-double` there.

**Before**

```js
test("web search off leaves the web-search app out of the turn; on or unsaid leaves the kernel to choose", async () => {
  const store = createStore();
  const off = await prepare(store, { clientRequestId: "request_web_1", message: "news today", webSearch: false });
  assert.deepEqual(off.inference.tool_scope, { exclude_apps: ["web-search"] });
  assert.equal(off.context.webSearch, "off");
  const on = await prepare(store, { clientRequestId: "request_web_2", message: "news today", webSearch: true });
  assert.equal(on.inference.tool_scope, undefined);
  const unsaid = await prepare(store, { clientRequestId: "request_web_3", message: "hello" });
  assert.equal(unsaid.inference.tool_scope, undefined);
```

**After**

```js
test("web search off leaves the web-search app out of the turn; on or unsaid leaves the kernel to choose", async () => {
  const store = createStore();
  // The Body double app is left out of every chat but a Body double
  // persona's (tests/body-double-chat.test.js).
  const off = await prepare(store, { clientRequestId: "request_web_1", message: "news today", webSearch: false });
  assert.deepEqual(off.inference.tool_scope, { exclude_apps: ["web-search", "body-double"] });
  assert.equal(off.context.webSearch, "off");
  const on = await prepare(store, { clientRequestId: "request_web_2", message: "news today", webSearch: true });
  assert.deepEqual(on.inference.tool_scope, { exclude_apps: ["body-double"] });
  const unsaid = await prepare(store, { clientRequestId: "request_web_3", message: "hello" });
  assert.deepEqual(unsaid.inference.tool_scope, { exclude_apps: ["body-double"] });
```

## 12. The Body double persona

In SayLLM, gear → Personas → New persona (`manage.js` `personaForm`). The task carries the
instructions, so nothing needs pasting.

| Field | Value |
|---|---|
| Name | Body double |
| Task | **Body double** (step 7) |
| Personality | Neutral (the task's instructions already set the tone) |
| Default reasoning | fast (the task suggests it). Move it up if the fast tier proves unreliable at tool calls. |
| Temperature | 0.6 (the task suggests it; the iOS app's sampler) |
| Custom instructions | none |
| Memory | "Memory enabled" controls only whether this persona's chats recall memories (`chat-lifecycle.js`). To keep the journal out of memories, untick the persona's **Universal memories** and **Persona memories** switches on its card: memory formation checks those (`memory-formation.js`), and Universal is on by default. SayLLM never stores a tool result, but the person's chat messages and the assistant's replies, which repeat what `now` and `day` return, can both form memories. Conversation summaries are written either way; only private turns are never read. |

The task's instruction, as `composePersonaPrompt` puts it first in the system prompt:

```text
You are the user's body double for interstitial journaling. Through the day they tell you what they are doing. You keep them oriented and keep their record with the body-double tools.
- When they start or switch to something, first call body-double__set_task with a short title. When they finish or drop it, call body-double__update_task with its id.
- Every time they say what they are doing or thinking, call body-double__log, with their own words as text and a one-line summary.
- When they mention something to come back to, call body-double__open_thread. When it is dealt with, call body-double__close_thread with its id.
- "Check on me in 25 minutes" is body-double__checkins with next_in_minutes.
- If you are not sure what is current, call body-double__now. A question is not an entry: answer it from body-double__now or body-double__day, and do not log it.
- Use ids exactly as the tools show them. When unsure, record less.
Reply in at most two short sentences, conversational and grounded. No flattery, no lectures, no emojis. Mention elapsed time or an open loose end only when it is genuinely useful. Ask a brief follow-up now and then, not every turn.
Asked for a recap of the day, read body-double__day and write four to six plain sentences: what got done, what is still open, any pattern worth noticing. No advice unless something obviously stalled. No lists.
```

The persona can only call the tools the person's grants allow. Someone without a grant on
`body-double` who picks the task gets the instructions but no tools.

## Checked so far

Against `jacepero/greenhouse` main at `d176a23`, in a scratch copy of its `llmplayground/` (the
Greenhouse checkout itself was not changed):

- **The Befores:** every "Before" above occurs exactly once in the real files and was replaced with
  its "After". The `src/http.js` anchor is a fragment of one long line.
- **SayLLM's own suite:** `node --test "tests/*.test.js"`, as `package.json` runs it, with both new
  test files added. Unpatched, 411 of 412 pass; patched, 451 of 452. The new tests pass:
  - the cards' 35;
  - the chat test's 5: a Body double persona's chat is offered the app and reserves room; other
    chats leave it out; web search's switch still applies; a long Body double chat is trimmed to
    the reserved budget, not the plain one; and the reserve setting is checked. The long-chat
    test fails if step 8's trim edit is left out.

  The one failure is the same test both times: it reads `deploy/greenhouse-llmplayground.service`,
  which a copy of `llmplayground/` alone does not have. `express` and `better-sqlite3` were small
  local stand-ins, since no packages could be installed.
- **Export in Chromium (Playwright):** the page was served from `/apps/llmplayground/` under
  SayLLM's own content security policy. It used the real patched `home.js`, `widgets.js`,
  `apps.js`, `styles.css`, `auth.js` and `ui-state.js` `lockedRequest`, as `app.js` wires them.
  - A good answer saved `body-double-2026-10-15.md` with the app's content.
  - A 403 put "Could not export: identity lacks 'read'" on the card and raised nothing.
  - A redirect to a login page became SayLLM's `AuthExpiredError` through `ctx.report`, engaged
    the lock and saved nothing. The next press sent no request, and the card said Greenhouse was
    locked.
- **Layout in the same page:**
  - Today's first row has no divider while "N earlier" is hidden;
  - rows align to the top;
  - Check-ins lines are 12.5px;
  - "Export" shows as a word.

  Earlier, at 375 and 390 px, the button also read "Export" on Home and on the page.

## Open points

- The tick (`checkins.tick`, every five minutes) writes one audited call each time. The owner's
  Activity card lists recent scheduled calls, so "Body double: checkins.tick" will appear there
  beside Home Assistant's house check. Whether Activity should hide no-op runs is a kernel and
  SayLLM question, not part of this change.
- Check-in notices reach only the owner, in SayLLM, the next time it is open (apps/body-double.md,
  "Who sees it"). Everyone else sees a due check-in on Focus and the assistant sees it in `now`.
- The tint (violet) and the icons are choices for the owner to confirm. `persona`, `report`, `pin`
  and `clock` exist in `icons.js`.
