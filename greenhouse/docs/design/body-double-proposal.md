# Greenhouse — Body Double proposal

**Status:** PROPOSAL. Nothing here is decided. The app is built (`body-double-app/`), so the
decisions below can be made against working code. Nothing is registered, approved, granted or
deployed.
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
| The person's words stored before any inference | An app tool can store first. A SayLLM widget can call it with no model involved. |
| A check-in chain every 45 minutes, quiet hours, eight ahead | Manifest `schedules` call a `command` tool on a cron (D9). The kernel has no per-person timer API and no quiet hours. |
| Local notifications with a reply box | `/api/notify` in-app notices. They are household-wide, read by the owner only, and shown by SayLLM for an allowlist of categories. There is no push transport until the iOS shell exists. |
| A daily recap by the model | The assistant can write one in chat from a read tool. D18 would allow an unprompted one. |
| Voice in and out | SayLLM's voice mode. |
| A Live Activity showing the current task | SayLLM Home widgets and rail pages, built on an app's read tools. |
| Export to JSON and Markdown | A read tool can return either. |

## The proposal

A registered app, `body-double`, built as a Rust crate like weather and web search (D4) in the
Greenhouse repository (D14).

- **It is a set of tools for the assistant, not a second assistant (D1).** The read tools are
  `now`, `day` and `export`. The write tools are `log`, `set_task`, `update_task`,
  `open_thread`, `close_thread` and `checkins`. "Ok, switching to the WGU assignment" becomes
  `set_task` plus `log`, then a one- or two-sentence reply. The tool descriptions carry the iOS
  prompt's tone, and they are part of the approved contract.
- **Check-ins are deterministic (D2) and kernel-scheduled (D9).** The `checkins.tick` schedule
  calls `checkin_tick` every five minutes. The app decides who is due from each person's plan
  (interval, quiet hours, zone), records the check-in, and raises a template notice. No model is
  involved. Answering is any activity, and it retires the notice. Eight unanswered in a row stop
  the chain until the person is back, as on iOS.
- **No model, no egress, no secrets.** The app talks only to the kernel on loopback. Inference
  happens in the assistant's own turns, on the kernel's routing (D6 applies there).
- **SayLLM shows it.** A "Body double" rail page holds four widgets: Focus, Today, Loose ends and
  Check-ins. The check-in category goes on SayLLM's notice allowlist. Every widget action is a
  direct tool call, so the narrate box stores the words with no model in the path.
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

These are read from Greenhouse source as of mid-October.

- **Every tool call is in the kernel's audit with its arguments.** Dispatch writes each call,
  allowed or refused, to `tool_audit` with the arguments' JSON. A `log` call therefore puts the
  person's words into `kernel.db` as well as the journal. On the appliance that is tier 1, not the
  passkey tier. Chat messages are already stored by SayLLM; the widget's narrate box adds the
  kernel copy. Private turns are the exception: the kernel keeps no arguments for them.
- **A private SayLLM turn still reaches the app, and the app is not told.** The owner answered
  "Allowed, log w/o details" for tools in private turns (chat, 2026-10-10). The kernel honours
  that, since its log keeps no arguments, but Body Double would still keep the words. That is not
  "effectively never having happened".
- **Tool schemas cost context in every turn, and SayLLM budgets for less.** The kernel offers
  every callable tool in every chat turn, with no relevance filtering. Body Double's nine come to
  about 780 tokens (chars/4) on an 8,192-token runtime. SayLLM reserves 512 tokens of overhead for
  the kernel's policy, serving line and all tool schemas
  (`GREENHOUSE_RESERVED_OVERHEAD_TOKENS`). Those already ran past 512 before this app, and only
  the 0.8 safety factor absorbs the rest. Adding Body Double could push a long conversation over
  the runtime's context, and the kernel then answers 400. The descriptions were cut to the
  minimum. qwen3:8b has also been sensitive to tool conditioning before.
- **Check-ins are at most five minutes late.** The tick runs every five minutes, like Home
  Assistant's house check, to keep scheduled calls in the audit and Activity view down. A
  per-minute tick would make them exact at 1,440 audited calls a day.

## Decisions for the owner

1. **Scope.** Should a body double be part of Greenhouse?
   - **Recommended:** yes, as the registered app above. If accepted, the owner may record it as
     a scope entry; agents do not edit the register.
   - Alternative: a SayLLM persona only. It is quick, but it gives no structured task or loose-end
     state and no check-ins.
2. **The contract.** The app proposes:
   - nine tools for people (three read, six write);
   - one `command` tool, `checkin_tick`. It is never offered in chat, and the app refuses any
     caller but the scheduler;
   - the schedule `checkins.tick`, every five minutes;
   - in-app notices in category `body-double-checkin`, sensitivity `private`;
   - no secrets, no egress, no `model_compute`.

   **Recommended:** approve as proposed.
3. **Grants.** **Recommended:** owner `[read, write]` on `body-double`, and no `command` for
   anyone (the scheduler needs none). Grant other people only when they ask.
4. **Where the journal lives.** It is personal and longitudinal: what someone did all day, with
   moods when they state them. **Recommended:**
   - now, the app's state directory on VM140 (`/var/lib/greenhouse/apps/body-double`), backed up
     with the other apps;
   - on the appliance, the passkey-unlocked tier-2 volume as soon as it exists;
   - keep entries until deleted. Deleting entries is not built yet; it would be the first
     follow-up.
5. **Check-in notices.** **Recommended:**
   - notices for the owner only (configuration `notices_for: ["owner"]`), with
     `body-double-checkin` added to SayLLM's shown categories;
   - other people see due check-ins on their own page and widget, because notices are
     household-wide and owner-only;
   - no push. None exists today, and the owner chose "no push" for the memory report.
6. **Model use outside a turn.** **Recommended:** none for now. Check-ins are templates, which
   matches the owner's choice for Home's briefing line (a template, no model call), and recaps
   come from the assistant when asked. A later option is an unprompted evening recap through
   D18's background capability, with its budget and audit.
7. **Moods.** **Recommended:**
   - store a mood only when the person states one;
   - never put one in a check-in, and mention one in a recap only if asked.

   This follows the memory policy's stance that sensitive topics are never used in ambient or
   proactive output. Entries reach SayLLM only as tool results, so the poisoning guard already
   keeps them from forming memories.
8. **Where it shows.** **Recommended:**
   - the "Body double" rail page;
   - the Focus widget on a new person's Home by default, and the other widgets on the page.
     Someone with a saved layout, the owner included, adds Focus from Add widgets;
   - Check-ins off Home by default.
9. **Private turns.** The owner's answer, "Allowed, log w/o details", covers the kernel's log,
   not what an app keeps. **Recommended:** extend it. A small kernel change tells apps a call
   comes from a private turn, and Body Double then stores nothing from one. It touches a
   reviewed kernel path.
   - Alternative: keep the current behaviour and say in the app doc that a private narration is
     kept.
   - Alternative: allow writes only from the page, not from chat.
10. **The journal in the kernel's tool audit.** **Recommended:**
    - accept it for now, since chat text is already in SayLLM's store and private turns keep no
      arguments;
    - later, give the page a data-plane route through the kernel proxy, which keeps no tool log,
      so the narrate box no longer goes through a tool.

    The kernel proxy's handling of browser writes was not verified here, so that route is a
    follow-up, not part of this build.
11. **SayLLM's context budget.** **Recommended:** before any grant, raise SayLLM's
    `GREENHOUSE_RESERVED_OVERHEAD_TOKENS` by about 800, from 512 to about 1,300, so that the
    added schemas come out of history, not out of the safety margin. Then check a long
    conversation on the live configuration.
    - Alternative: a SayLLM change that offers Body Double's tools only with the Body double
      persona.

## Delivery, after the answers

1. **Copy into the repository.** `body-double-app/` joins the workspace members, and
   `Cargo.lock` gains its entry: no new crate, only `rusqlite`, `axum`, `tokio`, `serde`,
   `serde_json` and `reqwest`, all already locked. The appliance Containerfile's
   `COPY body-double-app` line lands in the same commit; without it the image build cannot load
   the workspace. The other files are `apps/body-double.md`,
   this proposal, and the `deploy/` files.
2. **Check against the real kernel crate.**
   - Run `cargo test -p body-double-app` and
     `cargo clippy -p body-double-app --all-targets --no-deps -- -D warnings`.
   - The crate was built here against a reconstruction of the kernel's app SDK (`README.md`).
     The real crate is the test that counts.
3. **SayLLM.** Apply `llmplayground/INTEGRATION.md` and run SayLLM's tests. Raise the reserved
   overhead (decision 11), then deploy `public/` with the owner's OK.
4. **VM140.**
   - Build, then run `deploy/body-double/install-body-double.sh`.
   - The owner approves the contract and sets grants as `greenhouse-op-owner`
     (`deploy/body-double.md`).
   - Check read-only. Never create real check-ins on production to test.
   - Record the deployment in `deploy/DEPLOYMENT-STATUS.md`.
5. **Appliance.** Apply `appliance/INTEGRATION.md` and prove it in the lab VM with the layer
   check.

## Out of scope for this proposal

- Phone push (it waits on the iOS shell, O04).
- Notices for people other than the owner.
- Model-written check-ins.
- Editing or deleting entries.
- Calendar links.
- Voice changes.
- Moving the journal into SayLLM's database.
