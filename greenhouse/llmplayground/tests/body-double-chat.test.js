// The Body double app in chat: its tools are offered only under a persona
// with the Body double task, and only those turns reserve room for them.

import test from "node:test";
import assert from "node:assert/strict";
import { commitChat, prepareChat } from "../src/chat-lifecycle.js";
import { UnavailableEmbeddingAdapter } from "../src/embeddings.js";
import { createStore } from "../src/store.js";
import { loadConfig } from "../src/config.js";
import { estimateTokens } from "../src/trim.js";
import {
  BODY_DOUBLE_APP, BODY_DOUBLE_OVERHEAD_TOKENS, BODY_DOUBLE_TASK, TASKS, composePersonaPrompt,
} from "../public/persona-options.js";

const owner = "owner@example.test";
const budget = { contextTokens: 8192, reservedForOutput: 2048, reservedForOverhead: 512, estimatorSafetyFactor: 0.8 };

function prepare(store, body, inferenceBudget = budget) {
  return prepareChat({
    store,
    embeddingAdapter: new UnavailableEmbeddingAdapter(),
    ownerUser: owner,
    body,
    defaultContextLimit: 16_384,
    inferenceBudget,
  });
}

function bodyDoublePersona(store) {
  return store.createPersona(owner, {
    label: "Body double", task: BODY_DOUBLE_TASK, systemPrompt: "", preferredReasoning: "fast",
    memoryEnabled: false, contextLimit: 16384, temperature: 0.6,
  });
}

function withEnvironment(values, run) {
  const previous = Object.fromEntries(Object.keys(values).map((name) => [name, process.env[name]]));
  try {
    for (const [name, value] of Object.entries(values)) {
      if (value == null) delete process.env[name];
      else process.env[name] = value;
    }
    return run();
  } finally {
    for (const [name, value] of Object.entries(previous)) {
      if (value == null) delete process.env[name];
      else process.env[name] = value;
    }
  }
}

test("a Body double persona's chat offers the app and reserves room for its tools", async () => {
  const store = createStore();
  const personaId = bodyDoublePersona(store);
  const chat = await prepare(store, { clientRequestId: "request_bd_1", message: "switching to the WGU assignment", personaId });
  assert.equal(chat.inference.tool_scope, undefined, "nothing left out");
  const plain = await prepare(store, { clientRequestId: "request_bd_2", message: "hello" });
  assert.deepEqual(plain.inference.tool_scope, { exclude_apps: [BODY_DOUBLE_APP] });
  // The reserve is in the budget: a message that fits a plain chat by fewer
  // tokens than the reserve does not fit a Body double chat.
  const plainBudget = Math.floor((8192 - 2048 - 512) * 0.8);
  const bodyDoubleBudget = Math.floor((8192 - 2048 - 512 - BODY_DOUBLE_OVERHEAD_TOKENS) * 0.8);
  // Just over the Body double budget, well under the plain one.
  let between = "";
  while (estimateTokens(between) <= bodyDoubleBudget + 100) between += "plain words here ";
  assert.ok(estimateTokens(between) < plainBudget - 300);
  await assert.doesNotReject(prepare(store, { clientRequestId: "request_bd_3", message: between }));
  await assert.rejects(
    prepare(store, { clientRequestId: "request_bd_4", message: between, personaId }),
    (error) => error.status === 413,
  );
  store.close();
});

test("the reserve follows the configured value, and web search off still applies", async () => {
  const store = createStore();
  const personaId = bodyDoublePersona(store);
  const chat = await prepare(
    store,
    { clientRequestId: "request_bd_5", message: "hi", personaId, webSearch: false },
    { ...budget, bodyDoubleOverhead: 0 },
  );
  assert.deepEqual(chat.inference.tool_scope, { exclude_apps: ["web-search"] });
  assert.equal(chat.context.history.budget, Math.floor((8192 - 2048 - 512) * 0.8));
  store.close();
});

test("a long Body double chat is trimmed to the reserved budget, not the plain one", async () => {
  const reserved = Math.floor((8192 - 2048 - 512 - BODY_DOUBLE_OVERHEAD_TOKENS) * 0.8);
  const plainBudget = Math.floor((8192 - 2048 - 512) * 0.8);
  const said = "Done the intro paragraph, next the citations for section two. ".repeat(3);
  const results = {};
  for (const kind of ["body-double", "plain"]) {
    const store = createStore();
    const personaId = kind === "body-double" ? bodyDoublePersona(store) : undefined;
    let conversationId;
    for (let i = 0; i < 40; i++) {
      const turn = await prepare(store, {
        clientRequestId: `request_long_${kind}_${i}`, message: said,
        ...(personaId ? { personaId } : {}), ...(conversationId ? { conversationId } : {}),
      });
      conversationId = turn.conversationId;
      commitChat({ store, ownerUser: owner, body: { turnId: turn.turnId, status: "complete", assistant: said } });
    }
    const last = await prepare(store, {
      clientRequestId: `request_long_${kind}_last`, message: "status?", conversationId,
      ...(personaId ? { personaId } : {}),
    });
    results[kind] = last.context.history;
    store.close();
  }
  assert.equal(results["body-double"].budget, reserved);
  assert.ok(results["body-double"].estimatedTokens <= reserved, JSON.stringify(results["body-double"]));
  assert.equal(results.plain.budget, plainBudget);
  assert.ok(results.plain.estimatedTokens > reserved, "the plain chat keeps more, so the trim really differs");
});

test("a private turn leaves the Body double app out, even in a Body double chat", async () => {
  const store = createStore();
  const personaId = bodyDoublePersona(store);
  const open = await prepare(store, { clientRequestId: "request_bd_private_1", message: "starting the dishes", personaId });
  assert.equal(open.inference.tool_scope, undefined);
  const hidden = await prepare(store, {
    clientRequestId: "request_bd_private_2", message: "something private", personaId,
    conversationId: open.conversationId, private: true,
  });
  assert.equal(hidden.inference.greenhouse_private, true);
  assert.deepEqual(hidden.inference.tool_scope, { exclude_apps: [BODY_DOUBLE_APP] });
  store.close();
});

test("the Body double task carries the persona's instructions and no mood", () => {
  const task = TASKS.find((item) => item.id === BODY_DOUBLE_TASK);
  assert.equal(task.label, "Body double");
  const prompt = composePersonaPrompt({ task: BODY_DOUBLE_TASK });
  assert.match(prompt, /body-double__set_task/);
  assert.match(prompt, /body-double__log/);
  assert.doesNotMatch(prompt, /mood/i);
});

test("the Body double reserve is configurable and checked", () => {
  const development = {
    GREENHOUSE_MODE: "development",
    GREENHOUSE_APP_HOST: "127.0.0.1",
    GREENHOUSE_KERNEL_URL: "http://127.0.0.1:4000",
  };
  const load = (value) => withEnvironment({ ...development, GREENHOUSE_BODY_DOUBLE_OVERHEAD_TOKENS: value }, loadConfig);
  assert.throws(() => load("-1"), /GREENHOUSE_BODY_DOUBLE_OVERHEAD_TOKENS/);
  assert.throws(() => load("5000"), /Body double reserve/, "it must leave room for history");
  assert.equal(load("900").bodyDoubleOverheadTokens, 900);
  assert.equal(load(null).bodyDoubleOverheadTokens, BODY_DOUBLE_OVERHEAD_TOKENS);
});
