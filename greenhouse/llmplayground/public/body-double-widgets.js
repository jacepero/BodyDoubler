// The Body double app's cards, for Home and for its page in the rail
// (apps/body-double.md). A body double for interstitial journaling: what you
// are on and for how long, a box to say what you are doing, today's journal,
// the loose ends you mentioned, and how often it checks on you. Every control
// is one call to one of the app's tools, as the person, with no model in the
// path.
//
// widgets.js calls registerBodyDoubleWidgets() at its end and hands over its
// own helpers. This module imports nothing: headButton is private to
// widgets.js, and importing widgets.js from here would be a cycle that reaches
// its bindings before they exist. The pure helpers hold the logic, so the
// tests need no DOM (tests/body-double-widgets.test.js).

const APP = "body-double";
const tool = (name) => `${APP}.${name}`;

/** How often a check-in may come, in minutes: the choices the card offers. */
export const EVERY_CHOICES = [15, 30, 45, 60, 90];

/** Check-ins stop after this many unanswered in a row (the app's rule, the iOS chain's length). */
export const UNANSWERED_LIMIT = 8;

/** The most each box takes, in characters, as the app's tools check it. */
export const LIMITS = { text: 2000, title: 200, note: 300 };

// ---- pure helpers ------------------------------------------------------------------------

/** How long, in the app's own words: "0m", "45m", "2h", "2h 10m", "3d 4h". */
export function spanLabel(minutes) {
  const total = Math.max(0, Math.floor(Number(minutes) || 0));
  if (total < 60) return `${total}m`;
  if (total < 48 * 60) {
    const hours = Math.floor(total / 60);
    const rest = total % 60;
    return rest ? `${hours}h ${rest}m` : `${hours}h`;
  }
  const days = Math.floor(total / 1440);
  const hours = Math.floor((total % 1440) / 60);
  return hours ? `${days}d ${hours}h` : `${days}d`;
}

/** How long since `sinceTs` (Unix seconds) at `nowMs`; "" without a start. */
export function elapsedLabel(sinceTs, nowMs = Date.now()) {
  if (typeof sinceTs !== "number" || !Number.isFinite(sinceTs)) return "";
  return spanLabel((nowMs / 1000 - sinceTs) / 60);
}

/** A check-in in the notice's own words: "Still on <task>? 2h 10m in." */
export function checkinPrompt(title, elapsed) {
  if (!title) return "What are you working on?";
  return elapsed ? `Still on ${title}? ${elapsed} in.` : `Still on ${title}?`;
}

/**
 * What Focus shows, from `now`: the current task and how long it has run, a
 * check-in waiting to be answered, when the person last said anything, and
 * the paused tasks to go back to.
 */
export function focusView(now, nowMs = Date.now()) {
  const current = Number.isInteger(now?.current_task?.id) ? now.current_task : null;
  const task = current
    ? {
      id: current.id,
      title: String(current.title || ""),
      since: String(current.since || ""),
      sinceTs: typeof current.since_ts === "number" && Number.isFinite(current.since_ts) ? current.since_ts : null,
    }
    : null;
  const elapsed = task ? elapsedLabel(task.sinceTs, nowMs) || String(current.for || "") : "";
  const due = now?.checkins?.due === true;
  const quiet = now?.minutes_since_last_entry;
  let lastEntry = "";
  if (typeof quiet === "number" && Number.isFinite(quiet)) {
    lastEntry = quiet < 1 ? "Last entry just now" : `Last entry ${spanLabel(quiet)} ago`;
  }
  return {
    task,
    elapsed,
    due,
    dueText: due ? checkinPrompt(task?.title, elapsed) : "",
    lastEntry,
    paused: (Array.isArray(now?.paused_tasks) ? now.paused_tasks : [])
      .filter((paused) => Number.isInteger(paused?.id))
      .map((paused) => ({ id: paused.id, title: String(paused.title || "") })),
  };
}

/**
 * The day's entries from `day`, oldest first so the newest is last, each
 * { id, time, text, summary }. With a limit, only the newest that fit, one
 * place kept for the "N earlier" line: { rows, earlier, total, truncated }.
 */
export function journalRows(day, limit = Infinity) {
  const entries = (Array.isArray(day?.entries) ? day.entries : [])
    .filter((entry) => typeof entry?.text === "string")
    .sort((a, b) => (Number(a.at_ts) || 0) - (Number(b.at_ts) || 0))
    .map((entry) => ({
      id: entry.id,
      time: String(entry.time || ""),
      text: entry.text,
      // A summary that only repeats the words says nothing more.
      summary: entry.summary && entry.summary !== entry.text ? String(entry.summary) : "",
    }));
  const truncated = day?.truncated === true;
  const room = Number.isFinite(limit) ? Math.max(1, Math.floor(limit)) : Infinity;
  if (entries.length <= room) return { rows: entries, earlier: 0, total: entries.length, truncated };
  const rows = entries.slice(-Math.max(1, room - 1));
  return { rows, earlier: entries.length - rows.length, total: entries.length, truncated };
}

/** How many journal lines a card of `span` ([columns, rows]) holds: about two a row. */
export function journalLimit(span) {
  const rows = Array.isArray(span) && Number.isFinite(span[1]) ? span[1] : 4;
  return Math.max(2, rows * 2 - 1);
}

/** The open loose ends from `now`, oldest first: { items, more, total }. */
export function looseEnds(now) {
  const items = (Array.isArray(now?.open_threads) ? now.open_threads : [])
    .filter((thread) => Number.isInteger(thread?.id))
    .map((thread) => ({ id: thread.id, note: String(thread.note || ""), opened: String(thread.opened || "") }));
  const more = Number.isInteger(now?.more_open_threads) && now.more_open_threads > 0 ? now.more_open_threads : 0;
  return { items, more, total: items.length + more };
}

/**
 * The check-in plan in words: { known, on, every, due, lines }. `zone` is this
 * device's time zone; when the plan keeps another, the card says so, because
 * its times are in the plan's zone.
 */
export function checkinSummary(plan, zone = "") {
  if (!plan || typeof plan !== "object") {
    return { known: false, on: false, every: null, due: false, lines: ["Check-ins begin with your first entry."] };
  }
  const on = plan.on === true;
  const due = plan.due === true;
  const every = Number.isInteger(plan.every_minutes) ? plan.every_minutes : null;
  const unanswered = Number.isInteger(plan.unanswered) ? plan.unanswered : 0;
  const lines = [];
  if (!on) lines.push("Off");
  else if (unanswered >= UNANSWERED_LIMIT) lines.push(`Stopped after ${unanswered} unanswered. They start again when you next say something.`);
  else if (due) lines.push("One is waiting. Say anything to answer it.");
  else if (plan.next) lines.push(/^\d{1,2}:\d{2}$/.test(plan.next) ? `Next at ${plan.next}` : `Next ${plan.next}`);
  else lines.push("Nothing scheduled yet");
  lines.push(plan.quiet ? `Quiet ${String(plan.quiet).replace("-", "–")}` : "No quiet hours");
  if (plan.timezone && zone && plan.timezone !== zone) lines.push(`Times in ${plan.timezone}`);
  if (on && plan.notices === true) lines.push("Also sent as a Greenhouse notice");
  return { known: true, on, every, due, lines };
}

/** This device's time zone ("America/New_York"), or "" when the browser does not say. */
export function deviceZone() {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "";
  } catch {
    return "";
  }
}

/** A change to the check-ins, with this device's zone so the plan follows the person. */
export function checkinArgs(change, zone = deviceZone()) {
  return typeof zone === "string" && zone ? { ...change, timezone: zone } : { ...change };
}

/** A box's words, trimmed: { text, problem }. Empty text means nothing to send. */
export function boxText(value, max) {
  const text = String(value ?? "").trim();
  // The app counts characters, not UTF-16 units.
  const count = [...text].length;
  if (count > max) return { text: "", problem: `That is ${count} characters. The most is ${max}.` };
  return { text, problem: "" };
}

function errorText(error) {
  return String(error?.message || error || "unknown error");
}

// ---- shared between this app's cards -----------------------------------------------------

// This app's cards on screen, by their body, so an action on one reloads them
// all: a line said on Focus shows in Today at once.
const shown = new Map();

function remember(body, ctx) {
  for (const key of shown.keys()) if (!key.isConnected) shown.delete(key);
  shown.set(body, ctx);
}

// After an action the cards reload together, and each reload clears Home's
// own short cache. So they share one answer per tool for a moment instead of
// calling once each. Kept per caller (Home's callTool).
const answers = new WeakMap();

function read(ctx, name, args = {}) {
  let mine = answers.get(ctx.callTool);
  if (!mine) {
    mine = new Map();
    answers.set(ctx.callTool, mine);
  }
  const key = `${name} ${JSON.stringify(args)}`;
  const hit = mine.get(key);
  if (hit && Date.now() - hit.at < 3_000) return hit.promise;
  const promise = ctx.callTool(tool(name), args);
  mine.set(key, { at: Date.now(), promise });
  promise.catch(() => mine.delete(key));
  return promise;
}

function refreshAll(ctx) {
  answers.get(ctx.callTool)?.clear();
  ctx.refresh();
  for (const [body, other] of [...shown]) {
    if (!body.isConnected) shown.delete(body);
    else if (other !== ctx) other.refresh();
  }
}

/**
 * One write as the person: the tool `name` with `args`, or a function making
 * the call. Then every card of this app reloads; or, if it failed, the card
 * says why and its controls work again.
 */
async function act(ctx, what, name, args, controls = [], done = () => {}) {
  for (const control of controls) control.disabled = true;
  try {
    await (typeof name === "function" ? name() : ctx.callTool(tool(name), args));
  } catch (error) {
    ctx.report?.(error);
    ctx.note(`${what}: ${errorText(error)}`);
    for (const control of controls) control.disabled = false;
    return false;
  }
  done();
  ctx.note("");
  refreshAll(ctx);
  return true;
}

// What the person is typing outlives the card's refresh (every minute) and
// keeps its focus, so a narration is never lost halfway.
const drafts = new WeakMap();

function keepDraft(body, box) {
  const draft = drafts.get(body) || { value: "", focused: false };
  drafts.set(body, draft);
  box.value = draft.value;
  box.addEventListener("input", () => { draft.value = box.value; });
  box.addEventListener("focus", () => { draft.focused = true; });
  box.addEventListener("blur", () => { draft.focused = false; });
  return draft;
}

// ---- the cards ---------------------------------------------------------------------------

/**
 * Registers the Body double cards with widgets.js's own helpers:
 * { registerWidget, el, button, input, icon, headButton, fitRows }.
 */
export function registerBodyDoubleWidgets({ registerWidget, el, button, input, icon, headButton, fitRows }) {
  function row(className, ...children) {
    const node = el("div", className);
    node.append(...children.filter(Boolean));
    return node;
  }

  /**
   * A one-line box and its buttons. Each action is { label, iconName, extra,
   * hint, name, max, args(text), what }; the first with `submit` runs on Enter.
   */
  function boxForm(body, ctx, placeholder, actions) {
    const form = el("form", "bd-say");
    const box = input("", { placeholder });
    box.className = "tp-input";
    box.maxLength = Math.max(...actions.map((action) => action.max));
    box.setAttribute("aria-label", placeholder);
    box.setAttribute("autocomplete", "off");
    const draft = keepDraft(body, box);
    const controls = [];
    let busy = false;
    const send = async (action) => {
      if (busy) return;
      const { text, problem } = boxText(box.value, action.max);
      if (problem) {
        ctx.note(problem);
        return;
      }
      if (!text) return;
      busy = true;
      await act(ctx, action.what, action.name, action.args(text), controls, () => {
        box.value = "";
        draft.value = "";
      });
      busy = false;
      for (const control of controls) control.disabled = false;
    };
    form.append(box);
    for (const action of actions) {
      const control = button(action.iconName ? "" : action.label, `btn sm${action.extra ? ` ${action.extra}` : ""}`,
        action.submit ? null : () => void send(action));
      if (action.submit) control.type = "submit";
      if (action.iconName) {
        control.append(icon(action.iconName));
        control.setAttribute("aria-label", action.label);
      }
      control.title = action.hint || action.label;
      controls.push(control);
      form.append(control);
    }
    const onEnter = actions.find((action) => action.submit) || actions[0];
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      void send(onEnter);
    });
    return { form, box, draft };
  }

  /** Puts a box on its card, and the cursor back in it if it was there before the refresh. */
  function place(body, { form, box, draft }) {
    body.append(form);
    if (draft.focused) {
      box.focus();
      box.setSelectionRange?.(box.value.length, box.value.length);
    }
  }

  // ---- focus: the current task -----------------------------------------------------------

  registerWidget({
    id: "bd-focus",
    title: "Focus",
    icon: "persona",
    tint: "violet",
    needs: [tool("now")],
    span: { min: [3, 2], max: [8, 5], default: [4, 3] },
    // The Body double page shows each card at its "large" span (home.js,
    // renderPage). No layout saved before spans holds these ids.
    legacy: { large: [8, 3] },
    refreshMs: 60_000,
    load: (ctx) => read(ctx, "now"),
    render(now, body, ctx) {
      remember(body, ctx);
      const view = focusView(now);
      const small = ctx.size === "small";
      ctx.subtitle(view.due && small ? "Check-in due" : view.task?.since ? `since ${view.task.since}` : "");
      let done = null;
      if (view.task && ctx.can(tool("update_task"))) {
        done = headButton("Done", "check", () => act(ctx, "Could not mark it done", "update_task",
          { task_id: view.task.id, status: "done" }, [done]));
      }
      ctx.actions(done);
      if (view.due && !small) body.append(row("bd-due", icon("bell"), el("span", "", view.dueText)));

      const current = el("div", "bd-now");
      if (view.task) {
        const title = el("span", "bd-task", view.task.title);
        title.title = view.task.title;
        const elapsed = el("b", "bd-elapsed num", view.elapsed);
        current.append(title, elapsed);
        if (view.task.sinceTs !== null) {
          ctx.every(15_000, () => { elapsed.textContent = elapsedLabel(view.task.sinceTs) || view.elapsed; });
        }
      } else {
        current.append(el("span", "bd-task bd-none", "Nothing on right now"));
      }
      if (!small && view.lastEntry) current.append(el("span", "bd-meta", view.lastEntry));
      body.append(current);

      // Paused tasks to go back to: one tap makes it current again.
      if (!small && view.paused.length) {
        const paused = el("div", "bd-paused");
        paused.append(el("span", "", "Paused"));
        const canResume = ctx.can(tool("update_task"));
        for (const task of view.paused.slice(0, ctx.size === "large" ? 5 : 3)) {
          if (!canResume) {
            paused.append(el("span", "bd-paused-name", task.title));
            continue;
          }
          const back = button(task.title, "btn sm ghost", () => act(ctx, "Could not go back to it", "update_task",
            { task_id: task.id, status: "active" }, [back]));
          back.title = `Back to ${task.title}`;
          paused.append(back);
        }
        body.append(paused);
      }

      const canLog = ctx.can(tool("log"));
      const canSwitch = ctx.can(tool("set_task"));
      if (!canLog && !canSwitch) return;
      const actions = [];
      if (canLog) {
        actions.push({ label: "Log it", iconName: "arrow-up", extra: "go", submit: true, name: "log",
          max: LIMITS.text, args: (text) => ({ text }), what: "Could not log it" });
      }
      if (canSwitch) {
        actions.push({ label: "Switch", hint: "Make this the current task", name: "set_task",
          max: LIMITS.title, args: (title) => ({ title }), what: "Could not switch" });
      }
      place(body, boxForm(body, ctx, canLog ? "What are you doing?" : "What are you switching to?", actions));
    },
  });

  // ---- today: the journal ----------------------------------------------------------------

  registerWidget({
    id: "bd-journal",
    title: "Today",
    icon: "report",
    tint: "violet",
    needs: [tool("day")],
    span: { min: [3, 2], max: [6, 8], default: [4, 4] },
    legacy: { large: [4, 6] },
    refreshMs: 120_000,
    load: (ctx) => read(ctx, "day"),
    render(day, body, ctx) {
      remember(body, ctx);
      ctx.actions();
      const { rows, earlier, total, truncated } = journalRows(day, journalLimit(ctx.span));
      ctx.subtitle(total ? `${total}${truncated ? "+" : ""} ${total === 1 ? "entry" : "entries"}` : "");
      if (!rows.length) {
        body.append(row("empty-state", icon("chat"), el("span", "", "Nothing logged today.")));
        return;
      }
      // Oldest first, the newest last, as the day went.
      const list = el("div", "bd-list");
      if (earlier) list.append(el("p", "bd-earlier", `${earlier} earlier`));
      for (const entry of rows) {
        const text = el("span", "bd-text", entry.text);
        text.title = entry.text;
        const words = row("widget-row-name", text, entry.summary ? el("span", "muted", entry.summary) : null);
        list.append(row("widget-row bd-entry", el("span", "bd-time num", entry.time), words));
      }
      body.append(list);
      fitRows(list);
    },
  });

  // ---- loose ends ------------------------------------------------------------------------

  registerWidget({
    id: "bd-loose-ends",
    title: "Loose ends",
    icon: "pin",
    tint: "violet",
    needs: [tool("now")],
    span: { min: [3, 2], max: [6, 6], default: [4, 3] },
    legacy: { large: [4, 3] },
    refreshMs: 60_000,
    load: (ctx) => read(ctx, "now"),
    render(now, body, ctx) {
      remember(body, ctx);
      ctx.actions();
      const { items, more, total } = looseEnds(now);
      ctx.subtitle(total ? `${total} open` : "");
      if (!items.length) {
        body.append(row("empty-state", icon("check-circle"), el("span", "", "No loose ends.")));
      } else {
        const canClose = ctx.can(tool("close_thread"));
        const list = el("div", "bd-list");
        for (const thread of items) {
          const text = el("span", "bd-text", thread.note);
          text.title = thread.note;
          const words = row("widget-row-name", text, thread.opened ? el("span", "muted", `opened ${thread.opened}`) : null);
          let close = null;
          if (canClose) {
            close = button("", "icon-btn sm", () => act(ctx, "Could not close it", "close_thread", { thread_id: thread.id }, [close]));
            close.append(icon("check"));
            close.setAttribute("aria-label", `Close: ${thread.note}`);
            close.title = "Dealt with";
          }
          list.append(row("widget-row", words, close));
        }
        if (more) list.append(el("p", "bd-earlier", `${more} more`));
        body.append(list);
        fitRows(list);
      }
      if (ctx.can(tool("open_thread"))) {
        place(body, boxForm(body, ctx, "Something to come back to", [{
          label: "Add", iconName: "plus", submit: true, name: "open_thread",
          max: LIMITS.note, args: (note) => ({ note }), what: "Could not add it",
        }]));
      }
    },
  });

  // ---- check-ins -------------------------------------------------------------------------

  registerWidget({
    id: "bd-checkins",
    title: "Check-ins",
    icon: "clock",
    tint: "violet",
    needs: [tool("now")],
    span: { min: [3, 2], max: [6, 4], default: [3, 3] },
    legacy: { large: [4, 3] },
    refreshMs: 60_000,
    default: false,
    load: (ctx) => read(ctx, "now"),
    render(now, body, ctx) {
      remember(body, ctx);
      ctx.actions();
      const view = checkinSummary(now?.checkins, deviceZone());
      let words = "";
      if (view.known) words = !view.on ? "off" : view.every ? `every ${spanLabel(view.every)}` : "on";
      ctx.subtitle(words);
      if (ctx.can(tool("checkins"))) {
        const controls = [];
        // The device's zone goes with every change, so quiet hours and times
        // follow the person. A zone the app cannot load does not stop the change.
        const change = (args) => act(ctx, "Could not change check-ins", async () => {
          const zone = deviceZone();
          try {
            return await ctx.callTool(tool("checkins"), checkinArgs(args, zone));
          } catch (error) {
            if (!zone || !/^timezone/.test(errorText(error))) throw error;
            return ctx.callTool(tool("checkins"), checkinArgs(args, ""));
          }
        }, null, controls);
        const toggle = button("", "sw", () => change({ on: !view.on }));
        toggle.setAttribute("role", "switch");
        toggle.setAttribute("aria-checked", String(view.on));
        toggle.setAttribute("aria-label", "Check-ins");
        controls.push(toggle);
        body.append(row("bd-switch", toggle, el("span", "", "Check on me")));
        const choices = el("div", "segmented bd-every");
        choices.setAttribute("role", "group");
        choices.setAttribute("aria-label", "How often");
        for (const minutes of EVERY_CHOICES) {
          const chosen = view.every === minutes;
          const choice = button(spanLabel(minutes), `seg${chosen ? " on" : ""}`, () => {
            if (!chosen) change({ every_minutes: minutes });
          });
          choice.setAttribute("aria-pressed", String(chosen));
          choice.title = `Every ${spanLabel(minutes)}`;
          controls.push(choice);
          choices.append(choice);
        }
        body.append(choices);
      }
      for (const line of view.lines) body.append(el("p", "widget-line bd-meta", line));
    },
  });
}
