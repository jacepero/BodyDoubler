// The Body double app in chat: its tools are offered only under a persona
// with the Body double task, and only those turns reserve room for them.

import test from "node:test";
import assert from "node:assert/strict";
import { prepareChat } from "../src/chat-lifecycle.js";
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
