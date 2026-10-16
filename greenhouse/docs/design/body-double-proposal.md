# Greenhouse — Body Double proposal

**Status:** ANSWERED. The owner answered every decision in chat on 2026-10-16 ("The owner's
answers"). They are not in the register yet: the scope entry is the owner's to record. The app is
in the repository (`body-double-app/`).
Approving its contract, granting it and deploying it are the owner's acts at deployment
(`deploy/body-double.md`); none has happened.
**Owns:** the case for a body double in Greenhouse and the decisions it needs. The app's
behaviour, tools and manifest are in [`apps/body-double.md`](../../apps/body-double.md) (D15).

## What the owner asked for (2026-10-15, in chat; not in the register)

> "Can you take the ideas of body doubler and distill them down to an app that is compatible
> with my Greenhouse application/OS?"

"Body doubler" is the owner's July 2026 iOS app (`jacepero/BodyDoubler`,
`body-double-ios-plan.md`). It is a body-doubling partner for interstitial journaling. The person
narrates their day, and a small on-device model answers in a sentence or two. The model tracks
the current task and the open loose ends, and the app checks in when the person goes quiet.

No register scope entry covers this. S8 ("scheduled tasks, proactive recommendations") is the
nearest. So this is new scope, and it is the owner's to accept (decision 1).

## What the iOS app does, and what Greenhouse already has for it

| Idea from the iOS app | In Greenhouse today |
|---|---|
| A brief, grounded partner that answers each remark | The persistent assistant (D1), in SayLLM chat. It calls registered apps' tools. |
| Memory in SQLite; the model sees a context block each turn | An app's own store, read through its tools. |
| Tool calls constrained by a grammar; unknown tools dropped | Native tool calls through the kernel, with effects and grants; the app checks every argument. |
| The person's words stored before any inference | On the page, yes: a SayLLM widget calls the app's `log` tool with no model involved. In chat, SayLLM keeps the message, and a journal entry exists only if the model calls `log`. |
| A check-in chain every 45 minutes, quiet hours, eight ahead | Manifest `schedules` call a `command` tool on a cron (D9). People have the kernel's own timers and reminders (`greenhouse.set_timer`, `greenhouse.set_reminder`), but apps cannot create them, and they have no quiet hours and no chain. |
| Local notifications with a reply box | `/api/notify` in-app notices. They are household-wide, read by the owner only, and shown by SayLLM for an allowlist of categories. There is no push transport until the iOS shell exists. |
| A daily recap by the model | The assistant can write one in chat from a read tool. D18 would allow an unprompted one. |
| Voice in and out | SayLLM's voice mode. |
| A Live Activity showing the current task | SayLLM Home widgets and rail pages, built on an app's read tools. |
| Export to JSON and Markdown | A download route on the app, reached through the kernel's `/apps/<id>/` proxy, which relays `content-disposition`. A tool cannot do it: its result goes into the model's context whole. |

## The proposal

A registered app, `body-double`, built as a Rust crate like weather and web search (D4) in the
Greenhouse repository (D14).

- **It is a set of tools for the assistant, not a second assistant (D1).** The read tools are
  `now` and `day`. The write tools are `log`, `set_task`, `update_task`, `open_thread`,
  `close_thread` and `checkins`. "Ok, switching to the WGU assignment" becomes `set_task`, then
  `log`, then a one- or two-sentence reply. The tool descriptions carry the iOS prompt's tone, and
  they are part of the approved contract.
- **Only a Body double persona's chats use them.** SayLLM gets a persona task, "Body double",
  that carries the iOS prompt. Chats under such a persona are offered the app's tools and reserve
  room for them. Every other chat leaves the app out, through the kernel's tool scope, and pays
  nothing for it.
- **Check-ins are deterministic (D2) and kernel-scheduled (D9).** The `checkins.tick` schedule
  calls `checkin_tick` every five minutes. The app decides who is due from each person's plan
  (interval, quiet hours, zone) and records the check-in. For the owner it also raises a
  template notice that carries none of the person's words. No model is involved. Answering is any
  activity, and it retires the notice. Eight unanswered in a row stop the chain until the person
  is back, as on iOS.
- **No model, no egress, no secrets.** The app talks only to the kernel on loopback. Inference
  happens in the assistant's own turns, on the kernel's routing (D6 applies there).
- **SayLLM shows it.** A "Body double" rail page holds four widgets: Focus, Today, Loose ends and
  Check-ins. The check-in category goes on SayLLM's notice allowlist. Every widget action is a
  direct tool call, so the narrate box stores the words with no model in the path.
- **The journal downloads as a file.** The Today widget's Export button fetches
  `/apps/body-double/export` through the kernel's proxy: Markdown or JSON, for the person's own
  journal only. It is not a tool, so it never enters a chat turn or the tool audit.
- **Deploys like weather.** VM140 gets a unit, an egress set that allows only the kernel, and an
  installer that provisions the slot. The appliance image gets the same unit with its
  restored-state condition, sysusers and tmpfiles lines, and egress set.

## How it sits with the register

- **D1:** the app is a capability provider. The assistant stays the only voice.
- **D2:** who is due, quiet hours and the chain limit are code. The assistant only narrates
  results.
- **D4:** Rust, using only crates already in the workspace lock (`rusqlite` as node-monitor uses
  it).
- **D6:** the journal is longitudinal personal data. The app runs no inference. Chat turns that
  read it run wherever the kernel admits sensitive work.
- **D9:** the kernel owns the schedule. The app declares it and answers it.
- **D14:** a crate in the monorepo, not a new repository.
- **D15:** `apps/body-double.md` owns the manifest and internals.
- **D18:** not used. Decision 6 is whether it should be.

## Findings that bear on the decisions

These are read from Greenhouse source, main at `d176a23` (2026-10-14).

- **Every tool call is in the kernel's audit with its arguments.** Dispatch writes each call,
  allowed or refused, to `tool_audit` with the arguments' JSON. A `log` call therefore puts the
  person's words into `kernel.db` as well as the journal. On the appliance that is tier 1, not the
  passkey tier. Chat messages are already stored by SayLLM; the widget's narrate box adds the
  kernel copy. Private turns are the exception: the kernel keeps no arguments for them.
- **A private SayLLM turn still reaches the app, and the app is not told.** The owner answered
  "Allowed, log w/o details" for tools in private turns (chat, 2026-10-10). The kernel honours
  that, since its log keeps no arguments, but Body Double would still keep the words. That is not
  "effectively never having happened". (Check-in notices carry none of the person's words, so a
  loose end opened in a private turn is at least not repeated into the kernel's notices.)
- **The kernel does not cut tool results.** A tool's result goes into the model's context whole.
  So `day` returns a chat-sized day, as the iOS recap did: each entry's summary or its first 120
  characters, the newest that fit 5 KB, with counts of what it left out. A day at every limit stays
  under 8 KB, about 2,000 tokens. The whole journal is a download, not a tool.
- **Tool schemas cost context in every turn that offers them.** The kernel offers every callable
  tool in a chat turn, unless the turn's tool scope leaves an app out, with no relevance
  filtering. Body Double's eight people-facing schemas come to 2,761 bytes in the kernel's wire
  form (`[body-double] ` description prefix, `body-double__` names), about 690 tokens by chars/4,
  on an 8,192-token runtime.
  - SayLLM reserves overhead for the kernel's policy, serving line and all tool schemas
    (`GREENHOUSE_RESERVED_OVERHEAD_TOKENS`). That reserve is 512 tokens: SayLLM's default
    (`llmplayground/src/config.js`) and the deployed context policy (8,192 / 2,048 / 512 / 0.8 in
    `deploy/DEPLOYMENT-STATUS.md`). The live VM140 value was not read here.
  - The schemas already ran past 512 before this app, and only the 0.8 safety factor absorbed the
    rest. Offered in every chat, Body Double could push a long conversation over the runtime's
    context, and the kernel then answers 400.
  - So the descriptions were cut to the minimum, and decision 11 limits where the schemas are
    offered.
  - The 8,192-token runtime is the fast tier's `gemma4:12b` (`DEPLOYMENT-STATUS`, 2026-09-25); the
    heavy tier is `qwen3:30b`.
  - The tool-conditioning record of 2026-09-09 is about its predecessor, `qwen3:8b`, with ten
    tools offered. How Gemma behaves with more tools offered has not been measured.
- **Journal content can reach SayLLM's memories.** SayLLM never stores a tool result. But it
  does store the person's chat messages and the assistant's replies, and both can form memories:
  - when the assistant reads `now` or `day` and repeats an entry in its reply, that reply can be
    the basis of an "inferred" memory (`llmplayground/src/memory-formation.js`). Such a memory is
    universal by default;
  - the conversation's summary includes the assistant's replies too, and it is written whatever
    the memory switches say;
  - narration typed in chat is the person's own message and forms memories like any other.

  The ways to keep the journal out:
  - the Body double persona's Universal memories and Persona memories switches off ("Memory
    enabled" governs only recall). This stops memories, not summaries;
  - private turns, which memory formation never reads. These stop both;
  - an instruction not to repeat entries word for word. This helps but guarantees nothing.
- **Greenhouse already has reminders.** `greenhouse.set_reminder` and `set_timer` are offered to
  every person and are delivered in SayLLM with a chime and a card. Body Double's
  `checkins {next_in_minutes}` ("check on me in 25 minutes") overlaps them. With both offered, the
  model has to pick one. Decision 12 is about this.
- **Check-ins are at most five minutes late.** The tick runs every five minutes, like Home
  Assistant's house check, to keep scheduled calls in the audit and Activity view down. A
  per-minute tick would make them exact at 1,440 audited calls a day.

## The owner's answers

Given in chat on 2026-10-16, to the decisions below, in two messages. They are recorded here as the
owner gave them. None is a register entry; recording the scope entry there is the owner's to do.

| Decision | The owner's words | What it means |
|---|---|---|
| 1. Scope | "Registered app" | Body double is a registered app. |
| 7. Moods | "Drop it I don't remember what it does" | `log` takes no mood; nothing stores one. |
| 9. Private turns | First a question: "Why would you need to edit the kernel to delete the app data?" Then, with the rest: "the rest is approved" | No kernel change. SayLLM leaves the app out of private turns. Built. |
| 11. Context budget | "raise it when in use on the app" | SayLLM reserves room for the app's tools only in a Body double persona's chats, the only chats that offer them. Built. |
| 2–6, 8, 10, 12 | "And the rest is approved" | The recommendations as written stand. |
| Into the repository | "Yes bring it in" | Brought in on `feature/body-double`, from `a69f96d`. |

## Decisions for the owner

1. **Scope.** Should a body double be part of Greenhouse? *Answered (2026-10-16): a registered
   app.* Neither the register nor `docs/current-work.md` records it yet; both are the owner's to
   write. The owner asked for it to be brought in ("Yes bring it in"), and it is on
   `feature/body-double`.
2. **The contract.** The app proposes:
   - eight tools for people (two read, six write);
   - one `command` tool, `checkin_tick`, for the scheduler. The kernel would offer it in chat to
     anyone granted `command` on the app, so nobody should be (decision 3). The app refuses any
     caller but the scheduler;
   - the schedule `checkins.tick`, every five minutes;
   - in-app notices in category `body-double-checkin`, sensitivity `private`;
   - no secrets, no egress, no `model_compute`.

   The journal download is an app route, not part of the manifest: the kernel's proxy reaches any
   registered app, and the app requires a data-plane identity with `read`.

   **Recommended:** approve as proposed. *The owner's answer (chat, 2026-10-16): approved.* The kernel approval itself is
   the owner's act at deployment.
3. **Grants.** *The owner's answer (chat, 2026-10-16): approved; set at deployment.* **Recommended:** owner `[read, write]` on `body-double`, and no `command` for
   anyone (the scheduler needs none). Grant other people only when they ask. `read` alone shows
   the cards and allows the download.
4. **Where the journal lives.** It is personal and longitudinal: what someone did all day.
   **Recommended:**
   - now, the app's state directory on VM140 (`/var/lib/greenhouse/apps/body-double`), backed up
     with the other apps;
   - on the appliance, the passkey-unlocked tier-2 volume as soon as it exists. *The owner's
     answer (chat, 2026-10-16): approved.* Tier 2 now exists (`appliance/README.md`), but holds only SayLLM's data. So
     the vault helper must bind the app's state folder too, and the app needs a locked mode.
     Until then the appliance does not run the app (`apps/body-double.md`, "Not yet");
   - keep entries until deleted. Deleting entries is not built yet; it would be the first
     follow-up.
5. **Check-in notices.** *The owner's answer (chat, 2026-10-16): approved.* **Recommended:**
   - notices for the owner only (configuration `"notices_for": "owner"`), with
     `body-double-checkin` added to SayLLM's shown categories. The app takes one name there and
     refuses a list, so nobody else's check-ins can reach the owner's feed;
   - the text is a template with none of the person's words: "Still on your current task?
     2h 10m in." and "1 loose end open.", never a task title or a note. Notices are kept
     in the kernel's database whatever their sensitivity;
   - other people see due check-ins on their own page and widget, because notices are
     household-wide and owner-only;
   - no push. None exists today, and the owner chose "no push" for the memory report.
6. **Model use outside a turn.** *The owner's answer (chat, 2026-10-16): approved.* **Recommended:** none for now. Check-ins are templates, which
   matches the owner's choice for Home's briefing line (a template, no model call), and recaps
   come from the assistant when asked. A later option is an unprompted evening recap through
   D18's background capability, with its budget and audit.
7. **Moods.** *Answered (2026-10-16): dropped.* The iOS app let the model attach a short label
   of how the person felt, such as "tired", to an entry when they said it. `log` no longer takes
   one: an entry is the person's words and a one-line summary, and nothing else. How they said
   they felt stays in their own words, if they said it.
8. **Where it shows.** *The owner's answer (chat, 2026-10-16): approved.* **Recommended:**
   - the "Body double" rail page;
   - the Focus widget on a new person's Home by default, and the other widgets on the page.
     Someone with a saved layout, the owner included, adds Focus from Add widgets;
   - Check-ins off Home by default.
9. **Private turns.** *The owner's answer (chat, 2026-10-16): approved: the recommendation below; built.* The owner's
   answer, "Allowed, log w/o details", covers the kernel's log,
   not what an app keeps. A private SayLLM turn still reaches Body Double's tools, and the app
   cannot tell: the kernel keeps the turn's arguments out of its own log but does not pass the
   fact to the app. So the app would keep a private narration in the journal. It cannot delete it
   afterwards either, since it never knew which entries came from private turns.

   No kernel change is needed to prevent it. SayLLM knows a turn is private, since it sends
   `greenhouse_private` itself. It already leaves an app out of a turn through the kernel's tool
   scope when web search is switched off.
   - **Recommended:** SayLLM leaves the Body double app out of private turns too. A private
     message then never reaches the journal, as it never reaches memory. It is one more condition
     in the tool scope that decision 11's change already builds.
   - Alternative: keep the current behaviour, and say in the app doc that a private narration is
     kept.
10. **The journal in the kernel's tool audit.** *The owner's answer (chat, 2026-10-16): approved.* **Recommended:**
    - accept it for now, since chat text is already in SayLLM's store and private turns keep no
      arguments;
    - later, give the narrate box a data-plane route through the kernel proxy too, which keeps no
      tool log, so it no longer goes through a tool.

    The download already uses the data plane, for reads only. The kernel proxy's handling of
    browser writes was not verified here, so a write route is a follow-up, not part of this
    build.
11. **SayLLM's context budget.** *Answered (2026-10-16): raise it when the app is in use.*
    Built in SayLLM (`llmplayground/INTEGRATION.md`, steps 7 to 12):
    - a persona task, "Body double", carries the persona's instructions;
    - only chats under such a persona are offered the app's tools, and those turns reserve
      `GREENHOUSE_BODY_DOUBLE_OVERHEAD_TOKENS`, 720 by default, on top of the 512;
    - every other chat leaves the app out (`exclude_apps`), so it keeps its whole history budget
      and pays nothing for Body Double.

    SayLLM's own suite passes with it, plus five new tests, including one showing a long Body
    double chat is trimmed to the reserved budget. Comparisons, which have no persona, leave the
    app out too. Before any grant, read the live
    `GREENHOUSE_RESERVED_OVERHEAD_TOKENS` on VM140 (512 by default and in the recorded policy).
    Then check a long Body double conversation on the live configuration.
12. **"Check on me in 25 minutes."** *The owner's answer (chat, 2026-10-16): approved: the one-off stays.* `checkins {next_in_minutes}` and the kernel's
    `greenhouse.set_reminder` both fit that request.
    - Body Double's one-off is a check-in. Any activity answers it, it shows as due on the Focus
      card, and it counts toward the eight unanswered. Only the owner gets a notice for it.
    - A reminder reaches everyone, with SayLLM's chime and card, but it does not know whether the
      person has been active since.

    **Recommended:** keep the one-off, because it is part of the body-doubling loop.
    - Alternative: drop `next_in_minutes`, which saves a little schema, and let the assistant set
      a reminder.

## Delivery

1. **Into the repository.** *Done* on `feature/body-double`, from main at `a69f96d`.
   - `body-double-app/` joins the workspace members.
   - `Cargo.lock` gains only its own entry, with no new crate.
   - The appliance Containerfile copies the crate, so the image build can load the workspace.
   - Also added: `apps/body-double.md`, this proposal, the `deploy/` files, and the SayLLM side
     (the cards, the persona task, the per-chat reserve and tool scope, and the private-turn rule).
2. **Checks on that branch.** *Done.*
   - With the real kernel crate: `cargo test --locked -p body-double-app` gives 36 unit and 21
     integration tests passing.
   - `cargo clippy --locked -p body-double-app --all-targets --no-deps -- -D warnings` is clean,
     and `rustfmt --edition 2021 --check` passes.
   - `cargo build --release --locked -p body-double-app` builds.
   - SayLLM's suite, `node --test "tests/*.test.js"` after `npm ci`: 457 of 457 on the branch,
     against 416 of 416 on main.
   - Hosted CI was not run (`[skip ci]`).
3. **SayLLM.** Deploy it, front end and server, and restart, with the owner's OK.
4. **VM140.**
   - Build, then run `deploy/body-double/install-body-double.sh`.
   - The owner approves the contract and sets grants as `greenhouse-op-owner`
     (`deploy/body-double.md`).
   - Check read-only. Never create real check-ins on production to test.
   - Record the deployment in `deploy/DEPLOYMENT-STATUS.md`.
5. **Appliance.** After tier 2 can hold the journal (decision 4), the image edits listed in
   `apps/body-double.md`, "Not yet", proven in the lab VM with the layer check.

## Out of scope for this proposal

- Phone push. It waits on the iOS shell: the roadmap's "Richer iOS / Siri / desktop clients",
  planned, estimated as O04 in `docs/reviews/2026-09-14-remaining-work-and-appliance-loe-analysis.md`
  (analysis only). The kernel queues external deliveries until then, and S7 makes what push
  carries a user toggle.
- Notices for people other than the owner.
- Model-written check-ins.
- Editing or deleting entries.
- Calendar links.
- Voice changes.
- Moving the journal into SayLLM's database.
