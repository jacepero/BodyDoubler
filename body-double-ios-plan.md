# Body Double — iOS Local-AI Companion App

## Implementation Plan (handoff for Claude Code)

## 1. Project Summary

A native iOS app that acts as a "body doubling" partner for interstitial journaling. The user narrates what they're doing throughout the day; a small on-device LLM acknowledges, asks brief follow-ups, tracks current task/open threads, and persists everything to a local SQLite database. Voice output via built-in TTS. Check-ins are driven by scheduled local notifications (iOS does not permit long-running background processes).

**Explicit non-goals (v1):** cloud/network inference, encyclopedic model knowledge, always-listening background audio, multi-device sync. Connecting to a home inference server (DREAMSICLE) is a stretch goal, not v1.

**Core design principle:** the model is stateless per turn. All memory lives in SQLite. Each inference call receives a compact, app-assembled context block (current task, recent entries, open threads, elapsed time) and must emit a constrained tool call plus a short spoken response.

## 2. Tech Stack

| Layer | Choice | Notes |
|---|---|---|
| Language/UI | Swift + SwiftUI | iOS 17+ target |
| Inference | llama.cpp (via SPM package, e.g. `llama.cpp` Swift bindings) with Metal | Alternative: MLX Swift. Prefer llama.cpp for GBNF grammar support |
| Model | Qwen3 4B Instruct, Q4_K_M GGUF (~2.5 GB) | Fallback tier: Qwen3 1.7B for older devices |
| Output constraint | GBNF grammar forcing JSON tool-call schema | Eliminates malformed tool calls |
| Persistence | SQLite via GRDB | Simple, inspectable, no CloudKit |
| TTS | AVSpeechSynthesizer | Use enhanced/premium voices when installed |
| STT (input) | SFSpeechRecognizer (on-device mode) or iOS 17 dictation | Text input also supported |
| Check-ins | UNUserNotificationCenter scheduled local notifications | Interval configurable; notification actions for quick reply |
| Model delivery | Download weights on first launch (background URLSession) | Do NOT bundle weights in the binary |

## 3. Architecture

```
┌─────────────────────────────────────────┐
│ SwiftUI Views                           │
│  • Chat/entry view  • Timeline  • Tasks │
│  • Settings (interval, voice, model)    │
├─────────────────────────────────────────┤
│ SessionCoordinator                      │
│  assembles context block per turn:      │
│  { now, current_task, open_threads,     │
│    last_N_entries, minutes_since_last } │
├──────────────┬──────────────────────────┤
│ InferenceKit │ ToolExecutor             │
│ (llama.cpp,  │ applies tool calls to DB │
│  GBNF)       │                          │
├──────────────┴──────────────────────────┤
│ Store (GRDB/SQLite)                     │
├─────────────────────────────────────────┤
│ NotificationScheduler │ SpeechIO (TTS/STT)
└─────────────────────────────────────────┘
```

## 4. Data Schema (SQLite)

```sql
CREATE TABLE entries (
  id INTEGER PRIMARY KEY,
  created_at TEXT NOT NULL,        -- ISO 8601
  raw_text TEXT NOT NULL,          -- what the user said
  summary TEXT,                    -- model-generated one-liner
  task_id INTEGER,                 -- nullable FK
  mood TEXT,                       -- optional tag
  source TEXT NOT NULL             -- 'voice' | 'text' | 'notification_reply'
);

CREATE TABLE tasks (
  id INTEGER PRIMARY KEY,
  title TEXT NOT NULL,
  status TEXT NOT NULL,            -- 'active' | 'paused' | 'done' | 'dropped'
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  is_current INTEGER DEFAULT 0     -- at most one row = 1
);

CREATE TABLE threads (             -- open loops the model should nudge about
  id INTEGER PRIMARY KEY,
  note TEXT NOT NULL,
  status TEXT NOT NULL,            -- 'open' | 'closed'
  created_at TEXT NOT NULL,
  closed_at TEXT
);

CREATE TABLE checkins (
  id INTEGER PRIMARY KEY,
  scheduled_for TEXT NOT NULL,
  fired_at TEXT,
  responded INTEGER DEFAULT 0
);

CREATE TABLE app_state (           -- key/value: checkin_interval_min, voice_id, model_tier, etc.
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
```

## 5. Tool Schema (model output contract)

Every model response is a single JSON object matching this shape, enforced by GBNF:

```json
{
  "speech": "string — ≤2 sentences, spoken aloud via TTS",
  "actions": [
    { "tool": "log_entry",        "args": { "summary": "string", "mood": "string|null" } },
    { "tool": "set_current_task", "args": { "title": "string" } },
    { "tool": "update_task",      "args": { "task_id": 0, "status": "active|paused|done|dropped" } },
    { "tool": "open_thread",      "args": { "note": "string" } },
    { "tool": "close_thread",     "args": { "thread_id": 0 } },
    { "tool": "schedule_checkin", "args": { "minutes_from_now": 0 } },
    { "tool": "none",             "args": {} }
  ]
}
```

Rules for ToolExecutor:
- `actions` may contain 0–3 items; execute in order; unknown tool → drop silently, never crash.
- `log_entry` is implicit for every user utterance — the app always inserts the raw entry; the model's `log_entry` only supplies summary/mood enrichment.
- `set_current_task` clears any prior `is_current` flag.

## 6. Prompting

System prompt (~300 tokens): role ("brief, grounded body-doubling partner"), tone (no flattery, no lectures, short spoken replies), tool schema description, and the rule that it must respond with the JSON object only.

Per-turn user block assembled by SessionCoordinator:

```
[time] 2026-07-17T14:32 (43 min since last entry)
[current task] Refactor IVR callback handler (active, 2h 10m)
[open threads] (1) email Dana re: schema review
[recent]
- 13:49 started refactor after standup
- 12:10 lunch
[user says] "ok switching to the WGU assignment for a bit"
```

Keep total context under ~1,500 tokens for fast time-to-first-token on device.

## 7. Check-in Flow

1. `NotificationScheduler` posts a local notification at the configured interval (default 45 min), with a text-input notification action ("What are you up to?").
2. Reply from the notification → app processes in a background task window (`UNNotificationServiceExtension` not needed; handle via `userNotificationCenter(_:didReceive:)` + `beginBackgroundTask` for the short inference).
   - If inference can't complete in the background window on-device, fall back: store the entry immediately, run inference + TTS next foreground.
3. Opening the app cancels pending redundant check-ins and reschedules from now.
4. Quiet hours configurable in settings.

## 8. Build Phases

**Phase 1 — Skeleton + persistence (no AI)**
SwiftUI shell, GRDB store with schema above, manual text entry, timeline view, task list, notification check-ins with text replies. App is already a usable journaling tool at this point.

**Phase 2 — Inference**
Integrate llama.cpp SPM, model downloader with checksum + resume, GBNF grammar, SessionCoordinator context assembly, ToolExecutor. Bench on device: target <1.5 s to first token, <6 s full response on Qwen3 4B / recent hardware; auto-select 1.7B tier if the device has <6 GB RAM.

**Phase 3 — Voice**
AVSpeechSynthesizer output of `speech` field; SFSpeechRecognizer push-to-talk input; voice picker in settings.

**Phase 4 — Polish**
Daily summary view (model-generated end-of-day recap on demand), thread nudging ("Dana email is still open"), export (JSON/markdown dump), quiet hours, Live Activity showing current task (display only).

**Stretch (post-v1)**
Remote inference endpoint setting (OpenAI-compatible URL) for home-server use with on-device fallback.

## 9. Risks / Known Constraints

- **Backgrounding:** iOS suspends the app; there is no persistent agent. All "background" behavior = notifications + short background task windows. Do not attempt audio-session keep-alive hacks.
- **RAM:** 4B Q4 needs ~3 GB headroom; must degrade to 1.7B gracefully. Detect via `os_proc_available_memory()`.
- **Thermals/battery:** cap inference to user-initiated turns; no speculative generation.
- **App Store:** weights downloaded post-install; disclose on-device processing in privacy labels (all data stays local — this is a selling point).

## 10. Acceptance Criteria (v1)

- Cold start to interactive < 3 s (model lazy-loads on first turn).
- A notification reply results in a stored entry 100% of the time, even if inference fails.
- Tool calls are well-formed 100% of the time (grammar-enforced) and never crash ToolExecutor.
- All data readable via a single exported file; no network calls except model download.
