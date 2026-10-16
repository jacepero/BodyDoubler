# Greenhouse — App: Body Double

**Status:** Built (`body-double-app/`). Not deployed. Deployment is in
[`deploy/body-double.md`](../deploy/body-double.md); the decisions it needs are in
[`docs/design/body-double-proposal.md`](../docs/design/body-double-proposal.md).
**Owns:** the body double's behaviour, tools, storage and manifest.

## What it is

A body double for interstitial journaling, distilled from the owner's July 2026 iOS app
(`jacepero/BodyDoubler`). The person says what they are doing through the day. Greenhouse keeps
them oriented: the current task and how long it has run, the loose ends they mentioned, a journal
of what they said, and a check-in when they have gone quiet.

On iOS one small on-device model did everything. On Greenhouse the work splits along the
platform's lines:

| Body Double on iOS | Body Double on Greenhouse |
|---|---|
| On-device Qwen3 via llama.cpp, one stateless turn per utterance | The one persistent assistant, in SayLLM chat with a Body double persona (the persona task "Body double"). Only those chats offer this app's tools; the kernel routes inference. The app holds no model. |
| App-assembled context block (`[time] [current task] [open threads] [recent]`) | The `now` tool returns the same block as JSON. |
| `{speech, actions[≤3]}` under a GBNF grammar; unknown tools dropped | Native tool calls through the kernel. Each tool checks its own arguments strictly; an unknown tool is a 404. |
| Raw entry stored before inference, every time | Only on the page. The narrate box calls `log` directly, with no model in the path, so the words are stored first. In chat, SayLLM keeps the person's message, but a journal entry exists only if the model calls `log`, and its `text` is what the model sends. Nothing is journaled when inference fails. Entries do not record which path wrote them: the iOS `source` column was dropped, because both paths reach the app as the same kind of tool call. |
| SQLite (GRDB): entries, tasks, threads, check-ins, app_state | SQLite in the app's state directory, per person. |
| Local notification chain (8 ahead), quiet hours, reply from the notification | A kernel cron schedule calls `checkin_tick` every five minutes. The app decides who is due, respects quiet hours, and raises an in-app notice for the owner. Answering means logging anything. |
| Opening the app reset the check-in cadence | Not carried over. Only writes count as presence. Opening SayLLM, or reading `now` or `day`, answers nothing. |
| Daily recap by the model | `day` returns the day. The assistant writes the recap in chat. |
| Export (JSON, Markdown) | A download from the app's own route, `GET /apps/body-double/export`, through the kernel's proxy: the Export button on the Today card. It is not a tool, so the journal never goes into a chat turn. |
| TTS, push-to-talk, Live Activity | SayLLM already has voice. A Home widget shows the current task. |

## How a person uses it

- **In chat, with the Body double persona.** "ok, switching to the WGU assignment for a bit."
  The assistant calls `set_task`, then `log`, then answers in a sentence or two: "WGU it is. The
  IVR refactor is paused at 2h 10m." "What was I doing before lunch?" reads `now` or `day`. Other
  personas' chats do not offer this app's tools, so they pay nothing for them
  (`llmplayground/INTEGRATION.md`, steps 7 to 12).
- **On the Body double page in SayLLM** (rail entry and Home widgets, see
  [`llmplayground/`](../llmplayground/)): the current task with a live timer, a box to say what
  they are doing, Done and Switch buttons, today's journal with an Export button, loose ends and
  check-in settings. Every control except Export is a direct tool call.
- **Check-ins.** After `every_minutes` without activity, a check-in is raised. The owner also gets
  a notice: "Still on your current task? 45m in." Saying anything (a `log`, a task change)
  answers it and restarts the clock.

## Tools

Qualified as `body-double.<name>`. Every person's data is their own: the user in the kernel's
identity token (`u`) scopes every read and write. Ids are each person's own numbers (their first
task is 1, whoever else has tasks), shown by `now` and `day`. An id that is not one of the
caller's is "not found".

| Tool | Effect | Arguments |
|---|---|---|
| `now` | read | none |
| `day` | read | `date?` (`YYYY-MM-DD`, default today in the person's zone) |
| `log` | write | `text` (1–2000 chars, the person's words), `summary?` (≤200) |
| `set_task` | write | `title` (1–200) |
| `update_task` | write | `task_id` (integer), `status` (`active`, `paused`, `done`, `dropped`) |
| `open_thread` | write | `note` (1–300) |
| `close_thread` | write | `thread_id` (integer) |
| `checkins` | write | `on?` (bool), `every_minutes?` (10–240), `quiet?` (bool), `quiet_from?`, `quiet_until?` (`HH:MM`), `timezone?` (IANA name), `next_in_minutes?` (5–480) |
| `checkin_tick` | command | none. Scheduled only. |

Unknown argument names are refused, so a misspelt field is an error the model sees rather than
a silent default. Errors come back as `{"error": "..."}` with 400 (bad arguments), 404 (no such
task or loose end), 409 (a conflicting change) or 500 (storage).

**Tasks.** A task is `active`, `paused`, `done` or `dropped`. At most one is active, and the
active task is the current task.
- `set_task` pauses the previous current task and makes the named one current. It resumes a
  paused task of the same title, in any case and any script, rather than making a duplicate. A
  done or dropped task of that title is not resumed; a new one is made.
- `update_task` to `active` makes any of the person's tasks current again, a done or dropped one
  included. `done` and `dropped` end a task.
- An entry is filed under the task that is current when `log` runs. That is why `set_task`'s
  description says to call it before `log` when the person switches.

**Loose ends** (`threads`) are notes the person wants raised later. They stay open until closed.

**Activity.** Every write tool except `checkins` counts as the person being there: it answers an
open check-in, retires its notice and restarts the check-in clock. Reads do not count.

### Results

Times are the person's local time. A time on the same local day is `HH:MM`; an earlier one is
`Thu 15 Oct 14:32`. The times a live display needs have a `*_ts` twin in Unix seconds: a task's
`since`, an entry's `at` (in `now` and `log`), a loose end's `opened` and `closed`, and the next
check-in. `now.now` and the `time` fields in `day` have none.

`now`:

```json
{
  "now": "Thu 15 Oct 14:32",
  "timezone": "America/New_York",
  "current_task": {"id": 3, "title": "Refactor IVR callback handler", "status": "active",
                   "since": "12:22", "since_ts": 1792081320, "minutes": 130, "for": "2h 10m"},
  "minutes_since_last_entry": 43,
  "open_threads": [{"id": 1, "note": "email Dana re: schema review",
                    "opened": "Wed 14 Oct 16:05", "opened_ts": 1792008300}],
  "more_open_threads": 0,
  "paused_tasks": [{"id": 2, "title": "WGU assignment"}],
  "recent": [{"id": 9, "at": "13:49", "at_ts": 1792086540,
              "text": "started refactor after standup", "summary": null}],
  "checkins": {"on": true, "every_minutes": 45, "quiet": "22:00-08:00",
               "timezone": "America/New_York", "next": "15:17", "next_ts": 1792091820,
               "due": false, "unanswered": 0, "notices": true}
}
```

`current_task` is `null` when there is none, and `minutes_since_last_entry` is `null` before the
first entry. `recent` is the last five entries, oldest first, `text` cut to 160 characters.
`open_threads` is the oldest ten, and `more_open_threads` counts the rest. `paused_tasks` is the
five most recently touched. In `checkins`:
- `quiet` is `null` when the person has no quiet hours;
- `next` is `null` when nothing is scheduled;
- `due` means a check-in was raised and not yet answered;
- `timezone` is the zone in use, the person's own or the household's;
- `notices` says whether this person's check-ins also raise a Greenhouse notice, which is true
  only for the owner named in the configuration (`notices_for`).

`log`:

```json
{"logged": {"id": 10, "at": "14:32", "at_ts": 1792089120, "text": "ok switching to the WGU assignment",
            "summary": "Switched to the WGU assignment", "task": "WGU assignment"},
 "current_task": {"id": 2, "title": "WGU assignment", "status": "active", "since": "14:32",
                  "since_ts": 1792089120, "minutes": 0, "for": "0m"},
 "answered_checkin": false}
```

`set_task`: `{"current_task": {...}, "paused": {"id": 3, "title": "...", "minutes": 130} | null,
"resumed": false, "unchanged": false}`. `resumed` is true when a paused task of that title was
picked up again. `unchanged` is true when it was already the current task.

`update_task`: `{"task": {"id": 3, "title": "...", "status": "done"}, "current_task": {...} | null,
"paused": {...} | null}`.

`open_thread`: `{"thread": {"id": 4, "note": "...", "opened": "14:40", "opened_ts": 1792089600},
"open_threads": 2}`.

`close_thread`: `{"thread": {"id": 4, "note": "...", "opened": "14:40", "opened_ts": 1792089600,
"closed": "15:02", "closed_ts": 1792090920}, "open_threads": 1, "already_closed": false}`.

`day`:

```json
{
  "date": "2026-10-15", "label": "Thu 15 Oct", "timezone": "America/New_York",
  "entries": [{"time": "13:49", "text": "started refactor after standup",
               "task": "Refactor IVR callback handler"},
              {"time": "14:40", "text": "Switched to the WGU assignment",
               "task": "WGU assignment"}],
  "earlier": 0,
  "tasks": [{"id": 3, "title": "Refactor IVR callback handler", "status": "paused"}],
  "more_tasks": 0,
  "threads_opened": [{"id": 4, "note": "...", "time": "14:40"}],
  "more_threads_opened": 0,
  "threads_closed": [{"id": 1, "note": "...", "time": "15:02"}],
  "more_threads_closed": 0,
  "open_threads": [{"id": 4, "note": "..."}],
  "more_open_threads": 0,
  "checkins": {"raised": 3, "answered": 2},
  "truncated": false
}
```

`day` is sized for a chat turn, because the kernel puts a tool result into the model's context
whole, on an 8,192-token runtime. It follows the iOS recap, which took each entry's
`summary ?? text.prefix(120)`:
- each entry is its time, then its one-line summary or, without one, its words, cut to 120
  characters. `task` (cut to 60) appears only when the entry has one;
- `entries` are the day's newest, oldest first, as many as fit 5 KB of JSON (about 1,300
  tokens), at most 60. `earlier` counts the rest;
- `tasks` are the ten most recently touched that the day touched (logged against, created or
  changed), titles cut to 60;
- `threads_opened` and `threads_closed` are the day's newest five each, and `open_threads` the
  oldest five still open, notes cut to 80;
- each `more_*` counts what its list left out, and `truncated` is true when anything was.

Measured: a busy day of 60 short entries with summaries is 3.3 KB, all 60 included. A day at
every limit (2,000-character entries with 200-character summaries and titles, 40 loose ends) is
under 8 KB, about 2,000 tokens, with its newest 23 entries.

`checkins`: `{"checkins": {...the same object as in now...}}`.

`checkin_tick`: `{"due": 1, "raised": 1, "notified": 1, "deferred_for_quiet": 0, "paused": 0}`.
When a notice could not be sent, or a check-in could not be recorded, the tick still does
everything else. It then answers 502 (500 for storage), with the counts and each failure in
`error`, so the kernel's tool audit and Activity view show the call as failed.

### The journal download

`GET /export?format=markdown|json&from=YYYY-MM-DD&to=YYYY-MM-DD`, reached by a browser as
`/apps/body-double/export…` through the kernel's proxy. Every argument is optional: Markdown,
and the whole journal. It needs a data-plane identity (`p` `data`, subject `human`) with `read`,
so it is the person's own journal and nobody else's. Tool tokens are refused.

The answer is the file:
- `content-disposition: attachment; filename="body-double-2026-10-15.md"`;
- `cache-control: no-store`;
- `x-content-type-options: nosniff`.

The proxy relays all three. The file is at most 16 MiB; past that the route asks for a shorter
`from`/`to`. Unknown query names and a bad `format` are a 400.
- **Markdown:** a heading per day in the span, then a line per entry (time, words, summary,
  task), then all the tasks and loose ends. The person's text is escaped, so line breaks become spaces and
  Markdown and HTML syntax shows as written. A title such as `<img src=x onerror=…>` cannot run
  in a viewer.
- **JSON:** the entries and check-ins in the span, and every task and loose end, with UTC
  ISO-8601 times and the person's own ids, plus their check-in settings.

`from` and `to` limit the entries (and, in JSON, the check-ins) only. Both formats always hold
every task and loose end, whatever the span, as the iOS export did. A one-day file therefore still
lists every task title and loose-end note the person has.

It is not a tool. A tool's result goes into the model's context whole, and the kernel's tool
audit keeps its arguments. A download does neither, and changes nothing.

### Tool descriptions (part of the contract)

The descriptions are what steers the assistant, the way the iOS system prompt did ("at most two
short sentences, no flattery, no lectures"). They are in the manifest verbatim
(`body-double-app/src/lib.rs`, `manifest()`), so changing one is a contract change the owner
re-approves.

They are kept short on purpose. The kernel offers every callable tool in a chat turn unless the
turn's tool scope leaves an app out, with no relevance filtering. It sends each one with a
`[body-double] ` prefix on the description, under the name `body-double__<tool>`. In that form
the eight people-facing schemas come to 2,761 bytes of JSON, about 690 tokens by the chars/4
heuristic, on an 8,192-token runtime. SayLLM offers them only in a Body double persona's chats, and
reserves 720 tokens for them there (`GREENHOUSE_BODY_DOUBLE_OVERHEAD_TOKENS`). The parameter
schemas carry types, enums and lengths only; the app refuses anything else itself.

## Check-ins

Each person has a plan: on or off, every N minutes, quiet hours, a time zone, the next due time,
a requested one-off, and how many check-ins in a row went unanswered. The first write creates it
from the configuration's defaults (45 minutes, quiet 22:00–08:00, on). A person who never sets a
zone follows the configuration's, including a later change to it.

- **Activity** sets the next check-in to now + N minutes, moved to the end of quiet hours if it
  lands inside them, and resets the unanswered count.
- **The tick** runs every five minutes (`schedules: checkins.tick`, cron `*/5 * * * *`, the
  cadence of Home Assistant's house check). A check-in can be up to five minutes late. For each
  plan with a check-in due:
  - inside quiet hours, it moves the check-in to the end of them and raises nothing;
  - otherwise it records the check-in and moves the plan on in one transaction, then schedules
    the next one N minutes on.

  One person's failure does not stop the others'; that person's check-in is decided again at the
  next tick. Then the owner's notice goes out. A notice that did not go out (the kernel was down,
  say) is sent again at the next tick, for as long as the check-in is unanswered.
- **Eight unanswered in a row** (the iOS chain length) and the plan stops raising until the
  person is next active, or says `checkins {on: true}`. Nobody gets pinged all night or all
  weekend. Changing the interval or quiet hours does not restart a chain that stopped this way.
- `checkins {next_in_minutes}` asks for one at a set time ("check on me in 25 minutes"). It
  stands apart from the chain: activity does not move it, and it comes even with check-ins off.
  - Unlike the iOS one-off, which quiet hours moved like any other, it comes inside quiet hours
    too, because it was asked for at that time.
  - It counts toward the eight unanswered like any other check-in.
  - `next` shows whichever is sooner, the chain's or the one-off.
- `checkins {on: false}` stops the chain, cancels a requested one, and takes down an unread
  notice. `checkins {on: true}` that turns check-ins back on (they were off, or the chain had
  stopped after eight unanswered) also answers the open ones and takes the notice down. While the
  chain is still running it leaves them open; the next write answers them.

Quiet hours are local to the person's zone and may wrap midnight. A quiet end that falls in the
repeated hour of a DST change ends at its first occurrence. Times come from the kernel's clock
(the host's zone database). The kernel does not replay a cron minute missed while it was down.
The tick catches up on the next run, because it compares against stored due times.

**The notice.** Category `body-double-checkin`, sensitivity `private`, channel `in-app`, subject
`body-double/<user>/checkin`. A newer check-in replaces the older unread one, and answering
retires it. The text is a template, with no model involved and none of the person's words:
- with a current task: title `Check-in`, body `Still on your current task? 2h 10m in.`;
- without one: body `What are you working on?`;
- when loose ends are open, a second line: `2 loose ends open.`

No task title or loose-end note is ever in a notice. A notice is proactive output that the
kernel keeps in its own database (tier 1 on the appliance) whatever its sensitivity, so it carries
none of the person's words. The page and the assistant say what the task is. (SayLLM's proposed
memory policy, `sayllm-memory-and-sleep-proposal.md` §8, would also keep sensitive topics out of
proactive output. It is a proposal, not a register entry.)

**Who sees it.** Greenhouse notices are household-wide and only the owner reads them (kernel
`require_owner`). SayLLM shows only an allowlist of categories. So:
- only the owner's check-ins raise notices. `notices_for` names one person, the owner's user id,
  and a list is refused, so nobody else's check-ins reach the owner's feed;
- SayLLM shows the category once `body-double-checkin` is on its allowlist (`llmplayground/`);
- for everyone, a due check-in also shows on their own Body double page and widget (`due`), and
  the assistant sees it in `now`.

Phone push does not exist in Greenhouse yet. Notices reach the phone the next time SayLLM is
open.

## Storage

`$STATE_DIR/body-double.sqlite3` (default `/var/lib/greenhouse/apps/body-double`), WAL,
`PRAGMA user_version` migrations. All times are Unix seconds (UTC).

```sql
CREATE TABLE entries (id INTEGER PRIMARY KEY AUTOINCREMENT, user_id TEXT NOT NULL,
  seq INTEGER NOT NULL, at INTEGER NOT NULL, text TEXT NOT NULL, summary TEXT, task_id INTEGER);
CREATE TABLE tasks (id INTEGER PRIMARY KEY AUTOINCREMENT, user_id TEXT NOT NULL,
  seq INTEGER NOT NULL, title TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('active','paused','done','dropped')),
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, active_since INTEGER);
CREATE UNIQUE INDEX tasks_one_active ON tasks(user_id) WHERE status = 'active';
CREATE TABLE threads (id INTEGER PRIMARY KEY AUTOINCREMENT, user_id TEXT NOT NULL,
  seq INTEGER NOT NULL, note TEXT NOT NULL, opened_at INTEGER NOT NULL, closed_at INTEGER);
-- and a UNIQUE (user_id, seq) index on each of the three
CREATE TABLE checkins (id INTEGER PRIMARY KEY, user_id TEXT NOT NULL, raised_at INTEGER NOT NULL,
  answered_at INTEGER, notified INTEGER NOT NULL DEFAULT 0);
CREATE TABLE plans (user_id TEXT PRIMARY KEY, enabled INTEGER NOT NULL,
  every_minutes INTEGER NOT NULL, quiet_from INTEGER, quiet_until INTEGER,
  timezone TEXT NOT NULL, next_due_at INTEGER, oneoff_at INTEGER,
  unanswered INTEGER NOT NULL DEFAULT 0);
```

`seq` is the id people and the model see: each person's own count. If the shared row ids were
shown, one person could tell from the gaps how often someone else writes. Row ids stay inside the
app; `AUTOINCREMENT` keeps them from being reused after a delete. An entry's `task_id` is a task
row id, and it is only ever joined to the same person's tasks. `plans.timezone` is empty for a
person who never chose a zone.

The journal is personal and longitudinal. The proposal recommends the passkey-unlocked tier-2
volume once the appliance has one (decision 4, the owner's to answer).

## Manifest

The app registers this (built by `manifest()`):

```yaml
id: body-double
version: 0.1.0
health: /health
tools:
  - { name: now,          effect: read }
  - { name: day,          effect: read }
  - { name: log,          effect: write }
  - { name: set_task,     effect: write }
  - { name: update_task,  effect: write }
  - { name: open_thread,  effect: write }
  - { name: close_thread, effect: write }
  - { name: checkins,     effect: write }
  - { name: checkin_tick, effect: command }   # scheduled only
events: { emits: [], subscribes: [] }
schedules:
  - { name: checkins.tick, cron: "*/5 * * * *", action: checkin_tick }
permissions:
  secrets: []
  egress: []
  notify: [in-app]
  notify_categories: [{ category: body-double-checkin, sensitivity: private }]
```

No secrets, no egress, no `model_compute`, no `ui`. The download route needs no manifest entry:
the kernel's proxy reaches any registered app, and the app checks the identity itself. The app
talks only to the kernel on loopback: registration, `/api/notify` and `/api/notify/retire`.

People need `[read, write]` on `body-double`; `read` alone shows the cards and allows the
download. Nobody needs `command`. The scheduler calls `checkin_tick` as `system:scheduler`, and
the tool refuses any other caller. The kernel would offer it in chat to someone granted
`command`, which is one more reason to grant that to nobody.

## Configuration

`/etc/greenhouse/apps/body-double/config.json` (root-owned, 0640):

```json
{
  "version": "greenhouse-body-double-config-v1",
  "timezone": "America/New_York",
  "notices_for": "owner",
  "checkins": {"on": true, "every_minutes": 45, "quiet_from": "22:00", "quiet_until": "08:00"}
}
```

`timezone` is the default for people who have not set their own. `notices_for` is the owner's
user id: their check-ins also raise Greenhouse notices. It names one person, and a list is
refused (see "Who sees it"). `checkins` holds the defaults for a new plan; it and `notices_for`
are optional. Environment: `GREENHOUSE_KERNEL_URL` (default `http://127.0.0.1:4000`),
`GREENHOUSE_APP_PORT` (default `4107`), `GREENHOUSE_BODY_DOUBLE_CONFIG`,
`GREENHOUSE_BODY_DOUBLE_STATE_DIR`, and the registration credential through systemd
(`$CREDENTIALS_DIRECTORY/registration.credential`).

## Not yet

- **Phone push.** Waits on Greenhouse's iOS shell: the roadmap's "Richer iOS / Siri / desktop
  clients", planned. The kernel queues external deliveries until then.
- **Check-ins that reach non-owners as notices.** Notices are household-wide and owner-only.
- **A model-worded check-in or an evening recap without asking.** These would need
  `model_compute: [background]` (D18). The proposal recommends not now.
- **A reply box on the notice itself.** Today the person answers on the page or in chat.
- **Opening the page as presence**, as opening the iOS app was. Today only a write counts.
- **Editing and deleting entries.**
