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
| On-device Qwen3 via llama.cpp, one stateless turn per utterance | The one persistent assistant (SayLLM chat). It calls this app's tools; the kernel routes inference. The app holds no model. |
| App-assembled context block (`[time] [current task] [open threads] [recent]`) | The `now` tool returns the same block as JSON. |
| `{speech, actions[≤3]}` under a GBNF grammar; unknown tools dropped | Native tool calls through the kernel. Each tool checks its own arguments strictly; an unknown tool is a 404. |
| Raw entry stored before inference, every time | The `log` tool stores the person's words before anything else happens. The SayLLM page's narrate box calls `log` directly, with no model in the path. |
| SQLite (GRDB): entries, tasks, threads, check-ins, app_state | SQLite in the app's state directory, per person. |
| Local notification chain (8 ahead), quiet hours, reply from the notification | A kernel cron schedule calls `checkin_tick` every five minutes. The app decides who is due, respects quiet hours, and raises an in-app notice. Answering means logging anything. |
| Daily recap by the model | `day` returns the day. The assistant writes the recap in chat. |
| Export (JSON, Markdown) | `export` returns either. |
| TTS, push-to-talk, Live Activity | SayLLM already has voice. A Home widget shows the current task. |

## How a person uses it

- **In chat.** "ok, switching to the WGU assignment for a bit." The assistant calls `set_task`
  and `log`, then answers in a sentence or two: "WGU it is. The IVR refactor is paused at
  2h 10m." "What was I doing before lunch?" reads `now` or `day`.
- **On the Body double page in SayLLM** (rail entry and Home widgets, see
  [`llmplayground/`](../llmplayground/)): the current task with a live timer, a box to say what
  they are doing, Done and Switch buttons, today's journal, loose ends and check-in settings.
  Everything there is a direct tool call.
- **Check-ins.** After `every_minutes` without activity, a check-in is raised: "Still on WGU
  assignment? 45m in." Saying anything (a `log`, a task change) answers it and restarts the clock.

## Tools

Qualified as `body-double.<name>`. Every person's data is their own: the user in the kernel's
identity token (`u`) scopes every read and write. Ids are shown by `now` and `day`; an id that is
not the caller's is "not found".

| Tool | Effect | Arguments |
|---|---|---|
| `now` | read | none |
| `day` | read | `date?` (`YYYY-MM-DD`, default today in the person's zone) |
| `export` | read | `format?` (`markdown` default, or `json`), `from?`, `to?` (`YYYY-MM-DD`) |
| `log` | write | `text` (1–2000 chars, the person's words), `summary?` (≤200), `mood?` (≤40) |
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
active task is the current task. `set_task` pauses the previous current task and makes the named
one current, resuming an open task of the same title (case-insensitive) rather than making a
duplicate. `update_task` to `active` resumes a paused task the same way. `done` and `dropped`
end it.

**Loose ends** (`threads`) are notes the person wants raised later. They stay open until closed.

**Activity.** Every write tool except `checkins` counts as the person being there: it answers an
open check-in, retires its notice and restarts the check-in clock.

### Results

Times are the person's local time. A time on the same local day is `HH:MM`; an earlier one is
`Thu 15 Oct 14:32`. Every time has a `*_ts` twin in Unix seconds for live counters.

`now`:

```json
{
  "now": "Thu 15 Oct 14:32",
  "timezone": "America/New_York",
  "current_task": {"id": 3, "title": "Refactor IVR callback handler", "status": "active",
                   "since": "12:22", "since_ts": 1792089720, "minutes": 130, "for": "2h 10m"},
  "minutes_since_last_entry": 43,
  "open_threads": [{"id": 1, "note": "email Dana re: schema review",
                    "opened": "Wed 14 Oct 16:05", "opened_ts": 1792008300}],
  "more_open_threads": 0,
  "paused_tasks": [{"id": 2, "title": "WGU assignment"}],
  "recent": [{"id": 9, "at": "13:49", "at_ts": 1792086540,
              "text": "started refactor after standup", "summary": null}],
  "checkins": {"on": true, "every_minutes": 45, "quiet": "22:00-08:00",
               "timezone": "America/New_York", "next": "15:17", "next_ts": 1792095420,
               "due": false, "unanswered": 0, "notices": true}
}
```

`current_task` is `null` when there is none, and `minutes_since_last_entry` is `null` before the
first entry. `recent` is the last five entries, oldest first, `text` cut to 160 characters.
`open_threads` is the oldest ten, and `more_open_threads` counts the rest. `paused_tasks` is the
five most recently touched. In `checkins`, `quiet` is `null` when the person has no quiet hours,
`next` is `null` when nothing is scheduled, and `due` means a check-in was raised and not yet
answered. `notices` says whether this person's check-ins also raise a Greenhouse notice
(configuration `notices_for`).

`log`:

```json
{"logged": {"id": 10, "at": "14:32", "at_ts": 1792089120, "text": "ok switching to the WGU assignment",
            "summary": "Switched to the WGU assignment", "mood": null, "task": "WGU assignment"},
 "current_task": {"id": 2, "title": "WGU assignment", "status": "active", "since": "14:32",
                  "since_ts": 1792089120, "minutes": 0, "for": "0m"},
 "answered_checkin": false}
```

`set_task`: `{"current_task": {...}, "paused": {"id": 3, "title": "...", "minutes": 130} | null,
"resumed": false, "unchanged": false}`. `resumed` is true when an open task of that title was
picked up again. `unchanged` is true when it was already the current task.

`update_task`: `{"task": {"id": 3, "title": "...", "status": "done"}, "current_task": {...} | null,
"paused": {...} | null}`.

`open_thread`: `{"thread": {"id": 4, "note": "...", "opened": "14:40", "opened_ts": 1792089600},
"open_threads": 2}`.

`close_thread`: `{"thread": {"id": 4, "note": "...", "closed": "15:02", "closed_ts": 1792090920},
"open_threads": 1, "already_closed": false}`.

`day`:

```json
{
  "date": "2026-10-15", "label": "Thu 15 Oct", "timezone": "America/New_York",
  "entries": [{"id": 9, "time": "13:49", "at_ts": 1792086540, "text": "started refactor after standup",
               "summary": null, "mood": null, "task": "Refactor IVR callback handler"}],
  "tasks": [{"id": 3, "title": "Refactor IVR callback handler", "status": "active"}],
  "threads_opened": [{"id": 4, "note": "...", "time": "14:40"}],
  "threads_closed": [{"id": 1, "note": "...", "time": "15:02"}],
  "open_threads": [{"id": 4, "note": "..."}],
  "checkins": {"raised": 3, "answered": 2},
  "truncated": false
}
```

`tasks` are the tasks the day touched: logged against, created, or changed. At most 400 entries
are returned; `truncated` says when there were more.

`checkins`: `{"checkins": {...the same object as in now...}}`.

`export`: `{"format": "markdown", "filename": "body-double-2026-10-15.md", "entries": 212,
"content": "..."}`. The content is at most 512 KiB; past that the tool asks for a narrower
`from`/`to`. The JSON form holds every entry, task, loose end and check-in with UTC ISO-8601
times, plus the person's check-in settings.

`checkin_tick`: `{"due": 1, "raised": 1, "notified": 1, "deferred_for_quiet": 0, "paused": 0,
"errors": []}`.

### Tool descriptions (part of the contract)

The descriptions are what steers the assistant, the way the iOS system prompt did ("at most two
short sentences, no flattery, no lectures"). They are in the manifest verbatim
(`body-double-app/src/lib.rs`, `manifest()`), so changing one is a contract change the owner
re-approves.

They are kept short on purpose. The kernel offers every callable tool in every chat turn, with no
relevance filtering. The nine people-facing schemas come to about 3.1 KB of JSON, roughly 780
tokens by the chars/4 heuristic, on an 8,192-token runtime. The parameter schemas carry types,
enums and lengths only; the app refuses anything else itself.

## Check-ins

Each person has a plan: on or off, every N minutes, quiet hours, a time zone, the next due time,
a requested one-off, and how many check-ins in a row went unanswered. The first write creates it from the
configuration's defaults (45 minutes, quiet 22:00–08:00, on).

- **Activity** sets the next check-in to now + N minutes, moved to the end of quiet hours if it
  lands inside them, and resets the unanswered count.
- **The tick** runs every five minutes (`schedules: checkins.tick`, cron `*/5 * * * *`, the
  cadence of Home Assistant's house check). A check-in can be up to five minutes late. For each plan
  that is on and due: inside quiet hours it moves the check-in to the end of them and raises
  nothing. Otherwise it records a check-in, raises the notice if the person is in `notices_for`,
  and schedules the next one N minutes on.
- **Eight unanswered in a row** (the iOS chain length) and the plan stops raising until the
  person is next active. Nobody gets pinged all night or all weekend.
- `checkins {next_in_minutes}` asks for one at a set time ("check on me in 25 minutes"). Like the
  iOS one-off notification it stands apart from the chain: activity does not move it, quiet
  hours do not hold it, and it comes even with check-ins off. `next` shows whichever is sooner.
- `checkins {on: false}` stops the chain, cancels a requested one, and takes down an unread
  notice.

Quiet hours are local to the person's zone and may wrap midnight. Times come from the kernel's
clock (the host's zone database). The kernel does not replay a cron minute missed while it was
down. The tick catches up on the next run, because it compares against stored due times.

**The notice.** Category `body-double-checkin`, sensitivity `private`, channel `in-app`, subject
`body-double/<user>/checkin`, so a newer check-in replaces the older unread one and answering
retires it. Text, with no model involved:

- with a current task: title `Check-in`, body `Still on <task>? <2h 10m> in.`;
- without one: body `What are you working on?`;
- when a loose end is open, a second line: `Loose end: <oldest note>`.

Moods are never part of a check-in.

**Who sees it.** Greenhouse notices are household alerts addressed to the owner (kernel
`require_owner`), and SayLLM shows only an allowlist of categories. So:

- a person's check-ins raise notices only if they are in `notices_for` (meant for the owner);
- SayLLM shows the category once `body-double-checkin` is on its allowlist (`llmplayground/`);
- for everyone, a due check-in also shows on their own Body double page and widget (`due`), and
  the assistant sees it in `now`.

Phone push does not exist in Greenhouse yet. Notices reach the phone the next time SayLLM is
open.

## Storage

`$STATE_DIR/body-double.sqlite3` (default `/var/lib/greenhouse/apps/body-double`), WAL,
`PRAGMA user_version` migrations. All times are Unix seconds (UTC).

```sql
CREATE TABLE entries (id INTEGER PRIMARY KEY, user_id TEXT NOT NULL, at INTEGER NOT NULL,
  text TEXT NOT NULL, summary TEXT, mood TEXT, task_id INTEGER);
CREATE TABLE tasks (id INTEGER PRIMARY KEY, user_id TEXT NOT NULL, title TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('active','paused','done','dropped')),
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, active_since INTEGER);
CREATE UNIQUE INDEX tasks_one_active ON tasks(user_id) WHERE status = 'active';
CREATE TABLE threads (id INTEGER PRIMARY KEY, user_id TEXT NOT NULL, note TEXT NOT NULL,
  opened_at INTEGER NOT NULL, closed_at INTEGER);
CREATE TABLE checkins (id INTEGER PRIMARY KEY, user_id TEXT NOT NULL, raised_at INTEGER NOT NULL,
  answered_at INTEGER, notified INTEGER NOT NULL DEFAULT 0);
CREATE TABLE plans (user_id TEXT PRIMARY KEY, enabled INTEGER NOT NULL,
  every_minutes INTEGER NOT NULL, quiet_from INTEGER, quiet_until INTEGER,
  timezone TEXT NOT NULL, next_due_at INTEGER, oneoff_at INTEGER,
  unanswered INTEGER NOT NULL DEFAULT 0);
```

The journal is personal and longitudinal. It belongs on the passkey-unlocked tier-2 volume once
the appliance has one (proposal, decision 4).

## Manifest

The app registers this (built by `manifest()`):

```yaml
id: body-double
version: 0.1.0
health: /health
tools:
  - { name: now,          effect: read }
  - { name: day,          effect: read }
  - { name: export,       effect: read }
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

No secrets, no egress, no `model_compute`, no `ui`. The app talks only to the kernel on loopback:
registration, `/api/notify` and `/api/notify/retire`.

People need `[read, write]` on `body-double`. Nobody needs `command`: the scheduler calls
`checkin_tick` as `system:scheduler`, and the tool refuses any other caller.

## Configuration

`/etc/greenhouse/apps/body-double/config.json` (root-owned, 0640):

```json
{
  "version": "greenhouse-body-double-config-v1",
  "timezone": "America/New_York",
  "notices_for": ["owner"],
  "checkins": {"on": true, "every_minutes": 45, "quiet_from": "22:00", "quiet_until": "08:00"}
}
```

`timezone` is the default for people who have not set their own. `notices_for` lists the users
whose check-ins may raise Greenhouse notices. `checkins` holds the defaults for a new plan; it and
`notices_for` are optional. Environment: `GREENHOUSE_KERNEL_URL` (default
`http://127.0.0.1:4000`), `GREENHOUSE_APP_PORT` (default `4107`),
`GREENHOUSE_BODY_DOUBLE_CONFIG`, `GREENHOUSE_BODY_DOUBLE_STATE_DIR`, and the registration
credential through systemd (`$CREDENTIALS_DIRECTORY/registration.credential`).

## Not yet

- **Phone push.** Waits on Greenhouse's iOS shell (roadmap O04).
- **Check-ins that reach non-owners as notices.** Notices are household-wide and owner-only.
- **A model-worded check-in or an evening recap without asking.** These would need
  `model_compute: [background]` (D18). The proposal recommends not now.
- **A reply box on the notice itself.** Today the person answers on the page or in chat.
