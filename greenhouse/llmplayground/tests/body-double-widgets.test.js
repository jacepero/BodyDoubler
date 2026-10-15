// The Body double cards (public/body-double-widgets.js): their words for time,
// what Focus, Today, Loose ends and Check-ins show from the app's answers, and
// that each control makes exactly one tool call, and only when the person
// may make it. The cards render into a stand-in DOM of plain objects.

import { afterEach, test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import {
  EVERY_CHOICES, LIMITS, boxText, checkinArgs, checkinPrompt, checkinSummary, deviceZone, elapsedLabel, focusView,
  journalLimit, journalRows, looseEnds, registerBodyDoubleWidgets, spanLabel,
} from "../public/body-double-widgets.js";

// The tool results as apps/body-double.md gives them.
const NOW = {
  now: "Thu 15 Oct 14:32",
  timezone: "America/New_York",
  current_task: { id: 3, title: "Refactor IVR callback handler", status: "active", since: "12:22", since_ts: 1792089720, minutes: 130, for: "2h 10m" },
  minutes_since_last_entry: 43,
  open_threads: [{ id: 1, note: "email Dana re: schema review", opened: "Wed 14 Oct 16:05", opened_ts: 1792008300 }],
  more_open_threads: 0,
  paused_tasks: [{ id: 2, title: "WGU assignment" }],
  recent: [{ id: 9, at: "13:49", at_ts: 1792086540, text: "started refactor after standup", summary: null }],
  checkins: { on: true, every_minutes: 45, quiet: "22:00-08:00", timezone: "America/New_York", next: "15:17", next_ts: 1792095420, due: false, unanswered: 0, notices: true },
};
const DAY = {
  date: "2026-10-15", label: "Thu 15 Oct", timezone: "America/New_York",
  entries: [
    { id: 11, time: "14:40", at_ts: 1792089600, text: "switching to the WGU assignment", summary: "Switched to the WGU assignment", mood: null, task: "WGU assignment" },
    { id: 9, time: "13:49", at_ts: 1792086540, text: "started refactor after standup", summary: null, mood: null, task: "Refactor IVR callback handler" },
    { id: 10, time: "14:32", at_ts: 1792089120, text: "lunch", summary: "lunch", mood: null, task: null },
  ],
  tasks: [{ id: 3, title: "Refactor IVR callback handler", status: "active" }],
  threads_opened: [], threads_closed: [], open_threads: [],
  checkins: { raised: 3, answered: 2 },
  truncated: false,
};
const at = (ts, plusMinutes = 0) => (ts + plusMinutes * 60) * 1000;

// ---- a stand-in DOM and card --------------------------------------------------------------

const mounted = [];
afterEach(() => {
  // A card that is gone no longer reloads when another acts.
  for (const body of mounted.splice(0)) body.isConnected = false;
});

class FakeNode {
  constructor(tag, className = "", text = "") {
    Object.assign(this, { tag, className, textContent: text, children: [], attributes: {}, listeners: {} });
    Object.assign(this, { hidden: false, disabled: false, isConnected: true, value: "", title: "" });
  }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; }
  setAttribute(name, value) { this.attributes[name] = String(value); }
  getAttribute(name) { return this.attributes[name] ?? null; }
  addEventListener(type, listener) { (this.listeners[type] ||= []).push(listener); }
  fire(type) {
    const event = { type, target: this, currentTarget: this, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; } };
    // As in a browser, a disabled control is not clicked.
    if (type === "click" && this.disabled) return event;
    for (const listener of this.listeners[type] || []) listener(event);
    return event;
  }
  focus() {
    this.focused = true;
    this.fire("focus");
  }
}

function fakeDom() {
  const el = (tag, className = "", text = "") => new FakeNode(tag, className, text === null || text === undefined ? "" : String(text));
  const button = (label, className = "", onClick) => {
    const node = el("button", className, label);
    node.type = "button";
    if (onClick) node.addEventListener("click", onClick);
    return node;
  };
  const input = (value = "", { type = "text", placeholder = "" } = {}) => {
    const node = el("input");
    node.type = type;
    node.value = value ?? "";
    if (placeholder) node.placeholder = placeholder;
    return node;
  };
  const icon = (name) => Object.assign(el("svg", "ic"), { icon: name });
  // widgets.js's headButton, as it is.
  const headButton = (label, iconName, onClick, extra = "") => {
    const control = button("", `btn sm${extra ? ` ${extra}` : ""}`, onClick);
    if (iconName) control.append(icon(iconName));
    control.append(el("span", "lbl", label));
    if (iconName) control.setAttribute("aria-label", label);
    return control;
  };
  const fitted = [];
  const fitRows = (list) => { fitted.push(list); };
  return { el, button, input, icon, headButton, fitRows, fitted };
}

function setup() {
  const dom = fakeDom();
  const registry = new Map();
  registerBodyDoubleWidgets({ ...dom, registerWidget: (widget) => registry.set(widget.id, widget) });
  return { dom, registry };
}

/** A card's ctx as home.js gives it, recording what the card asks for. */
function fakeCtx({ can = () => true, answer = () => ({}), size = "medium", span = [4, 3] } = {}) {
  const ctx = {
    size,
    span,
    calls: [],
    notes: [],
    refreshes: 0,
    tickers: [],
    head: [],
    sub: null,
    callTool: async (qualified, args = {}) => {
      ctx.calls.push([qualified, args]);
      const result = answer(qualified, args);
      if (result instanceof Error) throw result;
      return result;
    },
    can: (qualified) => can(qualified),
    refresh: () => { ctx.refreshes += 1; },
    note: (text) => { ctx.notes.push(text); },
    report: () => {},
    every: (ms, fn) => { ctx.tickers.push([ms, fn]); },
    subtitle: (text) => { ctx.sub = text || ""; },
    actions: (...nodes) => { ctx.head = nodes.filter(Boolean); },
  };
  return ctx;
}

function show(registry, dom, id, data, ctx) {
  const body = dom.el("div", "wbody");
  mounted.push(body);
  registry.get(id).render(data, body, ctx);
  return body;
}

function all(node, test, found = []) {
  if (!node || typeof node !== "object") return found;
  if (test(node)) found.push(node);
  for (const child of node.children || []) all(child, test, found);
  return found;
}
const find = (node, test) => all(node, test)[0] || null;
const text = (node) => [node.textContent, ...(node.children || []).map(text)].filter(Boolean).join(" ");
const byClass = (name) => (node) => String(node.className || "").split(" ").includes(name);
const click = (node) => node.fire("click");
const settle = () => new Promise((resolve) => setImmediate(resolve));

// ---- time ---------------------------------------------------------------------------------

test("spans read the way the app writes them", () => {
  assert.equal(spanLabel(0), "0m");
  assert.equal(spanLabel(-5), "0m");
  assert.equal(spanLabel(45), "45m");
  assert.equal(spanLabel(120), "2h");
  assert.equal(spanLabel(130), "2h 10m");
  assert.equal(spanLabel(47 * 60 + 59), "47h 59m");
  assert.equal(spanLabel(2 * 1440), "2d");
  assert.equal(spanLabel(3 * 1440 + 4 * 60 + 5), "3d 4h");
  assert.equal(spanLabel("nonsense"), "0m");
});

test("the live counter counts whole minutes from the task's start", () => {
  const since = NOW.current_task.since_ts;
  assert.equal(elapsedLabel(since, at(since, 130)), "2h 10m");
  assert.equal(elapsedLabel(since, at(since, 130) + 59_000), "2h 10m", "a minute turns only when it is whole");
  assert.equal(elapsedLabel(since, at(since, 131)), "2h 11m");
  assert.equal(elapsedLabel(since, at(since, -2)), "0m", "a clock behind the server's never goes negative");
  assert.equal(elapsedLabel(null, Date.now()), "");
  assert.equal(elapsedLabel("1792089720", Date.now()), "", "only a number is a time");
});

// ---- what each card shows -----------------------------------------------------------------

test("Focus shows the current task, how long, and a check-in in the notice's words", () => {
  const view = focusView(NOW, at(NOW.current_task.since_ts, 130));
  assert.deepEqual(view.task, { id: 3, title: "Refactor IVR callback handler", since: "12:22", sinceTs: 1792089720 });
  assert.equal(view.elapsed, "2h 10m");
  assert.equal(view.due, false);
  assert.equal(view.dueText, "");
  assert.equal(view.lastEntry, "Last entry 43m ago");
  assert.deepEqual(view.paused, [{ id: 2, title: "WGU assignment" }]);

  const due = focusView({ ...NOW, checkins: { ...NOW.checkins, due: true } }, at(NOW.current_task.since_ts, 45));
  assert.equal(due.dueText, "Still on Refactor IVR callback handler? 45m in.");
  const idle = focusView({ ...NOW, current_task: null, checkins: { ...NOW.checkins, due: true }, minutes_since_last_entry: null });
  assert.equal(idle.task, null);
  assert.equal(idle.dueText, "What are you working on?");
  assert.equal(idle.lastEntry, "", "nothing said yet");
  assert.equal(checkinPrompt("WGU", ""), "Still on WGU?");
  const noStart = focusView({ ...NOW, current_task: { ...NOW.current_task, since_ts: null } });
  assert.equal(noStart.elapsed, "2h 10m", "without a start time, the app's own words");
  assert.deepEqual(focusView(null), { task: null, elapsed: "", due: false, dueText: "", lastEntry: "", paused: [] });
});

test("Today lists the day oldest first, so the newest is last, and keeps what fits", () => {
  const { rows, earlier, total } = journalRows(DAY);
  assert.deepEqual(rows.map((row) => row.time), ["13:49", "14:32", "14:40"]);
  assert.deepEqual(rows.map((row) => row.summary), ["", "", "Switched to the WGU assignment"], "no summary, or one that repeats the words, adds nothing");
  assert.equal(earlier, 0);
  assert.equal(total, 3);

  const many = { entries: Array.from({ length: 12 }, (_, i) => ({ id: i, time: `09:${String(i).padStart(2, "0")}`, at_ts: 1000 + i, text: `entry ${i}` })) };
  const cut = journalRows(many, 5);
  assert.deepEqual(cut.rows.map((row) => row.text), ["entry 8", "entry 9", "entry 10", "entry 11"], "the newest, with a line kept for the rest");
  assert.equal(cut.earlier, 8);
  assert.equal(cut.rows.length + 1, 5);
  assert.equal(journalRows({ entries: [], truncated: true }).truncated, true);
  assert.deepEqual(journalRows(null).rows, []);
  assert.equal(journalLimit([4, 2]), 3);
  assert.equal(journalLimit([4, 4]), 7);
  assert.equal(journalLimit([4, 6]), 11);
  assert.equal(journalLimit(undefined), 7);
});

test("Loose ends counts the ones now leaves out", () => {
  assert.deepEqual(looseEnds(NOW), { items: [{ id: 1, note: "email Dana re: schema review", opened: "Wed 14 Oct 16:05" }], more: 0, total: 1 });
  assert.equal(looseEnds({ ...NOW, more_open_threads: 4 }).total, 5);
  assert.deepEqual(looseEnds({}), { items: [], more: 0, total: 0 });
});

test("Check-ins say when the next one is, and the quiet hours", () => {
  const on = checkinSummary(NOW.checkins, "America/New_York");
  assert.equal(on.on, true);
  assert.equal(on.every, 45);
  assert.deepEqual(on.lines, ["Next at 15:17", "Quiet 22:00–08:00", "Also sent as a Greenhouse notice"]);
  const away = checkinSummary({ ...NOW.checkins, notices: false, next: "Fri 16 Oct 08:00" }, "Europe/Lisbon");
  assert.deepEqual(away.lines, ["Next Fri 16 Oct 08:00", "Quiet 22:00–08:00", "Times in America/New_York"]);
  assert.deepEqual(checkinSummary({ ...NOW.checkins, on: false, quiet: null, notices: false }, "").lines, ["Off", "No quiet hours"]);
  assert.match(checkinSummary({ ...NOW.checkins, due: true }).lines[0], /waiting/);
  assert.match(checkinSummary({ ...NOW.checkins, due: true, unanswered: 8 }).lines[0], /^Stopped after 8 unanswered/, "the app's due stays true");
  assert.deepEqual(checkinSummary(null), { known: false, on: false, every: null, due: false, lines: ["Check-ins begin with your first entry."] });
});

test("a change to check-ins carries this device's zone", () => {
  assert.deepEqual(checkinArgs({ on: false }, "America/Chicago"), { on: false, timezone: "America/Chicago" });
  assert.deepEqual(checkinArgs({ every_minutes: 30 }, ""), { every_minutes: 30 });
  assert.equal(typeof deviceZone(), "string");
  assert.deepEqual(EVERY_CHOICES, [15, 30, 45, 60, 90]);
  assert.ok(EVERY_CHOICES.every((minutes) => minutes >= 10 && minutes <= 240), "within what the tool takes");
});

test("a box sends trimmed words and refuses more than the tool takes", () => {
  assert.deepEqual(boxText("  on the refactor  ", LIMITS.text), { text: "on the refactor", problem: "" });
  assert.deepEqual(boxText("   ", LIMITS.text), { text: "", problem: "" });
  assert.equal(boxText("x".repeat(201), LIMITS.title).problem, "That is 201 characters. The most is 200.");
  assert.equal(boxText("🙂".repeat(300), LIMITS.note).problem, "", "characters, not UTF-16 units");
});

// ---- registration -------------------------------------------------------------------------

test("four cards register under Home's rules", () => {
  const { registry } = setup();
  assert.deepEqual([...registry.keys()], ["bd-focus", "bd-journal", "bd-loose-ends", "bd-checkins"]);
  // icons.js and the tint classes in styles.css, as of 2026-10-14.
  const icons = new Set(("arrow-up battery-low bell brain caret-down caret-right chat chats check check-circle clock cloud-moon "
    + "cloud-rain cloud-sun code cpu drop drop-half fingerprint gauge gear globe grip home image images info lamp leaf lightbulb lock "
    + "lock-simple microphone minus network paperclip persona pin plus power queue remind report reset search shield-check shuffle "
    + "sparkle speaker spinner squares sun thermometer timer wallet warning wind x cloud cloud-fog cloud-lightning cloud-snow moon").split(" "));
  const tints = new Set(["amber", "red", "skyc", "leafc", "teal", "cyan", "violet", "orange", "warnc"]);
  const within = ([columns, rows], min, max) => columns >= min[0] && columns <= max[0] && rows >= min[1] && rows <= max[1];
  for (const widget of registry.values()) {
    assert.match(widget.id, /^[a-z][a-z0-9-]{0,39}$/, "the layout API's id rule");
    assert.ok(icons.has(widget.icon), `${widget.id}: icon ${widget.icon}`);
    assert.ok(tints.has(widget.tint), `${widget.id}: tint ${widget.tint}`);
    assert.ok(widget.needs.every((qualified) => /^body-double\.(now|day)$/.test(qualified)), `${widget.id} needs only read tools`);
    const { min, max } = widget.span;
    assert.ok(max[0] <= 12 && max[1] <= 20, `${widget.id} fits the board`);
    assert.ok(within(widget.span.default, min, max), `${widget.id}: default span`);
    assert.ok(within(widget.legacy.large, min, max), `${widget.id}: page span`);
    assert.equal(typeof widget.load, "function");
    assert.equal(typeof widget.render, "function");
  }
  assert.deepEqual(registry.get("bd-focus").needs, ["body-double.now"]);
  // Only Focus is on a new person's Home; the rest are on the Body double page.
  assert.equal(registry.get("bd-focus").default, undefined);
  for (const id of ["bd-journal", "bd-loose-ends", "bd-checkins"]) {
    assert.equal(registry.get(id).default, false, `${id}: not on a new person's Home`);
  }
});

test("each card loads with one read", async () => {
  const { registry } = setup();
  const ctx = fakeCtx({ answer: (qualified) => (qualified === "body-double.day" ? DAY : NOW) });
  assert.equal(await registry.get("bd-focus").load(ctx), NOW);
  assert.equal(await registry.get("bd-loose-ends").load(ctx), NOW, "the same answer, shared");
  assert.equal(await registry.get("bd-journal").load(ctx), DAY);
  assert.deepEqual(ctx.calls, [["body-double.now", {}], ["body-double.day", {}]]);
});

// ---- Focus --------------------------------------------------------------------------------

test("Focus: saying what you are doing logs it, and the cards reload", async () => {
  const { registry, dom } = setup();
  const ctx = fakeCtx({ answer: () => ({ logged: { id: 10 } }) });
  const body = show(registry, dom, "bd-focus", NOW, ctx);
  assert.equal(text(find(body, byClass("bd-task"))), "Refactor IVR callback handler");
  assert.match(ctx.sub, /^since 12:22$/);
  const box = find(body, (node) => node.tag === "input");
  assert.equal(box.placeholder, "What are you doing?");
  box.value = "  started the refactor after standup ";
  const event = find(body, (node) => node.tag === "form").fire("submit");
  assert.equal(event.defaultPrevented, true, "the page does not navigate");
  await settle();
  assert.deepEqual(ctx.calls, [["body-double.log", { text: "started the refactor after standup" }]]);
  assert.equal(ctx.refreshes, 1);
  assert.equal(box.value, "", "the box is cleared for the next line");
  assert.deepEqual(ctx.notes, [""]);
});

test("Focus: an empty box sends nothing, a long one says why", async () => {
  const { registry, dom } = setup();
  const ctx = fakeCtx();
  const body = show(registry, dom, "bd-focus", NOW, ctx);
  const box = find(body, (node) => node.tag === "input");
  const form = find(body, (node) => node.tag === "form");
  box.value = "   ";
  form.fire("submit");
  box.value = "x".repeat(LIMITS.text + 1);
  form.fire("submit");
  await settle();
  assert.deepEqual(ctx.calls, []);
  assert.deepEqual(ctx.notes, ["That is 2001 characters. The most is 2000."]);
});

test("Focus: Switch makes the words the current task, Done ends it", async () => {
  const { registry, dom } = setup();
  const ctx = fakeCtx();
  const body = show(registry, dom, "bd-focus", NOW, ctx);
  find(body, (node) => node.tag === "input").value = "WGU assignment";
  click(find(body, (node) => node.tag === "button" && node.textContent === "Switch"));
  await settle();
  assert.deepEqual(ctx.calls.at(-1), ["body-double.set_task", { title: "WGU assignment" }]);
  const [done] = ctx.head;
  assert.equal(text(done), "Done");
  click(done);
  assert.equal(done.disabled, true, "one tap, one call");
  await settle();
  assert.deepEqual(ctx.calls.at(-1), ["body-double.update_task", { task_id: 3, status: "done" }]);
  assert.equal(ctx.refreshes, 2);
});

test("Focus: a paused task goes back to being current with one tap", async () => {
  const { registry, dom } = setup();
  const ctx = fakeCtx();
  const body = show(registry, dom, "bd-focus", NOW, ctx);
  click(find(body, (node) => node.tag === "button" && node.textContent === "WGU assignment"));
  await settle();
  assert.deepEqual(ctx.calls, [["body-double.update_task", { task_id: 2, status: "active" }]]);
});

test("Focus: a failed log keeps the words and says why", async () => {
  const { registry, dom } = setup();
  const ctx = fakeCtx({ answer: () => new Error("storage is unavailable") });
  const body = show(registry, dom, "bd-focus", NOW, ctx);
  const box = find(body, (node) => node.tag === "input");
  box.value = "on the call with Dana";
  find(body, (node) => node.tag === "form").fire("submit");
  await settle();
  assert.deepEqual(ctx.notes, ["Could not log it: storage is unavailable"]);
  assert.equal(ctx.refreshes, 0, "a reload would throw the words away");
  assert.equal(box.value, "on the call with Dana");
  assert.ok(all(body, (node) => node.tag === "button").every((control) => !control.disabled), "and the buttons work again");
});

test("Focus: the timer counts on its own, and a waiting check-in shows", () => {
  const { registry, dom } = setup();
  const ctx = fakeCtx();
  const started = Math.floor(Date.now() / 1000) - 75 * 60;
  const now = { ...NOW, current_task: { ...NOW.current_task, since_ts: started }, checkins: { ...NOW.checkins, due: true } };
  const body = show(registry, dom, "bd-focus", now, ctx);
  const elapsed = find(body, byClass("bd-elapsed"));
  assert.equal(elapsed.textContent, "1h 15m");
  elapsed.textContent = "";
  const [[ms, tick]] = ctx.tickers;
  assert.ok(ms <= 60_000);
  tick();
  assert.equal(elapsed.textContent, "1h 15m");
  assert.equal(text(find(body, byClass("bd-due"))), "Still on Refactor IVR callback handler? 1h 15m in.");
  const small = fakeCtx({ size: "small", span: [3, 2] });
  const smallBody = show(registry, dom, "bd-focus", now, small);
  assert.equal(find(smallBody, byClass("bd-due")), null, "a small card says it in its header");
  assert.equal(small.sub, "Check-in due");
});

test("Focus: with no current task there is nothing to finish", () => {
  const { registry, dom } = setup();
  const ctx = fakeCtx();
  const body = show(registry, dom, "bd-focus", { ...NOW, current_task: null, paused_tasks: [] }, ctx);
  assert.deepEqual(ctx.head, []);
  assert.equal(text(find(body, byClass("bd-task"))), "Nothing on right now");
  assert.ok(find(body, (node) => node.tag === "input"), "the box is still there to start one");
});

test("Focus: what is being typed survives the card's refresh, cursor and all", () => {
  const { registry, dom } = setup();
  const ctx = fakeCtx();
  const body = show(registry, dom, "bd-focus", NOW, ctx);
  const box = find(body, (node) => node.tag === "input");
  box.focus();
  box.value = "half a thou";
  box.fire("input");
  // home.js empties the body and renders the next answer into it.
  body.replaceChildren();
  registry.get("bd-focus").render(NOW, body, ctx);
  const again = find(body, (node) => node.tag === "input");
  assert.notEqual(again, box);
  assert.equal(again.value, "half a thou");
  assert.equal(again.focused, true, "the cursor is back where it was");
  again.fire("blur");
  body.replaceChildren();
  registry.get("bd-focus").render(NOW, body, ctx);
  const third = find(body, (node) => node.tag === "input");
  assert.equal(third.value, "half a thou");
  assert.equal(third.focused, undefined, "left alone once the person had moved on");
});

// ---- permissions --------------------------------------------------------------------------

test("without write grants the cards only show", () => {
  const { registry, dom } = setup();
  const readOnly = (qualified) => qualified === "body-double.now" || qualified === "body-double.day";
  const focus = fakeCtx({ can: readOnly });
  const focusBody = show(registry, dom, "bd-focus", NOW, focus);
  assert.deepEqual(focus.head, [], "no Done");
  assert.equal(find(focusBody, (node) => node.tag === "form"), null, "no box");
  assert.equal(find(focusBody, (node) => node.tag === "button"), null, "no Switch, no paused buttons");
  assert.match(text(find(focusBody, byClass("bd-paused"))), /WGU assignment/, "paused tasks still listed");

  const loose = fakeCtx({ can: readOnly });
  const looseBody = show(registry, dom, "bd-loose-ends", NOW, loose);
  assert.equal(find(looseBody, (node) => node.tag === "button"), null, "no close buttons");
  assert.equal(find(looseBody, (node) => node.tag === "form"), null, "no add box");

  const checkins = fakeCtx({ can: readOnly });
  const checkinsBody = show(registry, dom, "bd-checkins", NOW, checkins);
  assert.equal(find(checkinsBody, (node) => node.tag === "button"), null, "no switch, no choices");
  assert.match(text(checkinsBody), /Next at 15:17/);

  const logOnly = fakeCtx({ can: (qualified) => readOnly(qualified) || qualified === "body-double.log" });
  const logBody = show(registry, dom, "bd-focus", NOW, logOnly);
  assert.ok(find(logBody, (node) => node.tag === "form"), "a box to log");
  assert.equal(find(logBody, (node) => node.textContent === "Switch"), null, "but no Switch");
});

// ---- Today, Loose ends, Check-ins ---------------------------------------------------------

test("Today shows time, words and summary, newest last, fitted to the card", () => {
  const { registry, dom } = setup();
  const ctx = fakeCtx({ span: [4, 4] });
  const body = show(registry, dom, "bd-journal", DAY, ctx);
  const rows = all(body, byClass("bd-entry"));
  assert.deepEqual(rows.map((row) => text(find(row, byClass("bd-time")))), ["13:49", "14:32", "14:40"]);
  assert.equal(text(find(rows[2], byClass("muted"))), "Switched to the WGU assignment");
  assert.equal(ctx.sub, "3 entries");
  assert.deepEqual(dom.fitted, [find(body, byClass("bd-list"))]);
  const empty = show(registry, dom, "bd-journal", { ...DAY, entries: [] }, fakeCtx());
  assert.match(text(empty), /Nothing logged today/);
});

test("Loose ends: close one, or add one", async () => {
  const { registry, dom } = setup();
  const ctx = fakeCtx();
  const body = show(registry, dom, "bd-loose-ends", { ...NOW, more_open_threads: 2 }, ctx);
  assert.equal(ctx.sub, "3 open");
  assert.match(text(body), /2 more/);
  const close = find(body, (node) => node.getAttribute?.("aria-label") === "Close: email Dana re: schema review");
  click(close);
  await settle();
  assert.deepEqual(ctx.calls, [["body-double.close_thread", { thread_id: 1 }]]);
  const box = find(body, (node) => node.tag === "input");
  box.value = "book the dentist";
  find(body, (node) => node.tag === "form").fire("submit");
  await settle();
  assert.deepEqual(ctx.calls.at(-1), ["body-double.open_thread", { note: "book the dentist" }]);
  assert.equal(ctx.refreshes, 2);
  const none = show(registry, dom, "bd-loose-ends", { ...NOW, open_threads: [] }, fakeCtx());
  assert.match(text(none), /No loose ends/);
});

test("Check-ins: the switch and the choices send the device's zone", async () => {
  const { registry, dom } = setup();
  const ctx = fakeCtx();
  const body = show(registry, dom, "bd-checkins", NOW, ctx);
  assert.equal(ctx.sub, "every 45m");
  const toggle = find(body, byClass("sw"));
  assert.equal(toggle.getAttribute("aria-checked"), "true");
  click(toggle);
  assert.ok(all(body, (node) => node.tag === "button").every((control) => control.disabled), "one change at a time");
  await settle();
  const zone = deviceZone();
  assert.deepEqual(ctx.calls, [["body-double.checkins", zone ? { on: false, timezone: zone } : { on: false }]]);
  assert.equal(ctx.refreshes, 1);
  // The card reloads with the plan as it now is.
  const next = fakeCtx();
  const again = show(registry, dom, "bd-checkins", NOW, next);
  const choices = all(again, byClass("seg"));
  assert.deepEqual(choices.map((choice) => choice.textContent), ["15m", "30m", "45m", "1h", "1h 30m"]);
  assert.equal(choices[2].getAttribute("aria-pressed"), "true");
  click(choices[2]);
  await settle();
  assert.deepEqual(next.calls, [], "the chosen one sends nothing");
  click(choices[1]);
  await settle();
  assert.deepEqual(next.calls, [["body-double.checkins", zone ? { every_minutes: 30, timezone: zone } : { every_minutes: 30 }]]);
  assert.match(text(body), /Quiet 22:00–08:00/);
  const off = fakeCtx();
  show(registry, dom, "bd-checkins", { ...NOW, checkins: { ...NOW.checkins, on: false } }, off);
  assert.equal(off.sub, "off");
});

test("Check-ins: a zone the app cannot load does not stop the change", async () => {
  const { registry, dom } = setup();
  const ctx = fakeCtx({ answer: (_, args) => (args.timezone ? new Error("timezone: no such zone") : { checkins: {} }) });
  const body = show(registry, dom, "bd-checkins", NOW, ctx);
  click(find(body, byClass("sw")));
  await settle();
  const last = ctx.calls.at(-1);
  assert.deepEqual(last, ["body-double.checkins", { on: false }]);
  assert.equal(ctx.calls.length, deviceZone() ? 2 : 1);
  assert.deepEqual(ctx.notes, [""]);
  const failing = fakeCtx({ answer: () => new Error("storage is unavailable") });
  click(find(show(registry, dom, "bd-checkins", NOW, failing), byClass("sw")));
  await settle();
  assert.deepEqual(failing.notes, ["Could not change check-ins: storage is unavailable"]);
  assert.equal(failing.calls.length, 1, "only a zone problem is tried again");
});

test("an action on one card reloads the others on screen", async () => {
  const { registry, dom } = setup();
  const focus = fakeCtx();
  const journal = fakeCtx({ span: [4, 4] });
  const gone = fakeCtx();
  show(registry, dom, "bd-focus", NOW, focus).isConnected = true;
  show(registry, dom, "bd-journal", DAY, journal);
  show(registry, dom, "bd-loose-ends", NOW, gone).isConnected = false;
  const body = mounted[0];
  find(body, (node) => node.tag === "input").value = "lunch";
  find(body, (node) => node.tag === "form").fire("submit");
  await settle();
  assert.equal(focus.refreshes, 1);
  assert.equal(journal.refreshes, 1, "Today shows the new line at once");
  assert.equal(gone.refreshes, 0, "a card no longer on screen is left alone");
});

// ---- discipline ---------------------------------------------------------------------------

test("the module builds nodes only, and imports nothing", async () => {
  const source = await readFile(new URL("../public/body-double-widgets.js", import.meta.url), "utf8");
  assert.doesNotMatch(source, /innerHTML|outerHTML|insertAdjacentHTML/);
  assert.doesNotMatch(source, /^import /m, "widgets.js hands it its helpers");
  assert.doesNotMatch(source, /\bdocument\./, "the DOM comes through el, button and input");
});
