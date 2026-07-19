# Body Double

A native iOS "body doubling" companion for interstitial journaling. You narrate
what you're doing through the day; a small **on-device** LLM (Qwen3 via
llama.cpp + Metal) acknowledges, tracks your current task and open threads, and
persists everything to a local SQLite database. Voice out via `AVSpeechSynthesizer`,
push-to-talk in via on-device `SFSpeechRecognizer`, check-ins via scheduled local
notifications. **No cloud inference, no sync, no analytics — the only network
call the app ever makes is the one-time model download.**

Implements [body-double-ios-plan.md](body-double-ios-plan.md). Read that first;
this README covers how the code maps to it and how to build.

## Building (requires a Mac)

This source tree was authored off-Mac, so it has **not been compiled yet** —
expect a short first-build fix-up pass, concentrated in `InferenceEngine.swift`
(see "llama.cpp API drift" below).

1. Install Xcode 15.4+ (iOS 17 SDK) and [XcodeGen](https://github.com/yonaskolb/XcodeGen)
   (`brew install xcodegen`).
2. `cd BodyDoubler && xcodegen generate`
3. Open `BodyDouble.xcodeproj`, set your signing team on the `BodyDouble` and
   `BodyDoubleWidgets` targets.
4. **Pin the llama.cpp package**: in `project.yml` the `llama` package tracks
   `master`. Replace with a pinned `revision:` once the first build succeeds,
   then regenerate.
5. Build & run on a device (the simulator has no Metal GPU offload and no
   meaningful inference performance; everything else works there).
6. Tests: `Cmd-U` runs `BodyDoubleTests` (ToolExecutor / response parsing /
   store invariants — no model required).

### llama.cpp API drift

`InferenceEngine.swift` targets the 2026 C API: `llama_model_load_from_file`,
`llama_init_from_model`, `llama_model_get_vocab`, `llama_memory_clear`,
`llama_vocab_is_eog`, `llama_sampler_*`. The llama.cpp project renames C
symbols occasionally; if the pinned revision differs, the compiler errors point
at exactly which call to rename (older equivalents are noted in comments, e.g.
`llama_kv_cache_clear`). All llama.cpp usage is confined to that one file.

### Model checksums

`ModelCatalog.swift` ships with **empty `sha256` fields, which skips
verification**. Before shipping: download each GGUF once, compute
`shasum -a 256 <file>`, and paste the digests in. The download URLs point at
the official Qwen GGUF repos on Hugging Face — verify they still resolve.

## Architecture map (plan §3 → code)

| Plan component | File(s) |
|---|---|
| SwiftUI views | `BodyDouble/Views/*` — Chat ("Now"), `JournalTimelineView` (+ daily recap), Tasks (+ threads), Settings |
| SessionCoordinator | `BodyDouble/Session/SessionCoordinator.swift` — context block assembly (§6), turn pipeline, daily summary |
| InferenceKit | `BodyDouble/Inference/InferenceEngine.swift` (llama.cpp actor), `ToolGrammar.swift` (GBNF, §5), `ModelCatalog.swift` + `ModelDownloader.swift` (§2 model delivery) |
| ToolExecutor | `BodyDouble/Session/ToolExecutor.swift` + `ModelResponse.swift` (tolerant JSON decode) |
| Store (GRDB/SQLite) | `BodyDouble/Store/Store.swift` (schema §4 verbatim), `Records.swift`, `AppSettings.swift` (app_state) |
| NotificationScheduler | `BodyDouble/Notifications/NotificationScheduler.swift` (§7: chain of 8, quiet hours), `NotificationDelegate.swift` (text-input replies) |
| SpeechIO | `BodyDouble/Speech/SpeechIO.swift` (TTS + on-device STT) |
| Live Activity (§8 P4) | `BodyDouble/LiveActivity/*`, `BodyDoubleWidgets/` (display-only) |
| Export (§8 P4) | `BodyDouble/Export/Exporter.swift` (JSON + Markdown) |
| Composition root | `BodyDouble/App/AppEnvironment.swift`, `BodyDoubleApp.swift` |

## How the plan's guarantees are enforced

- **Entry stored 100% of the time (§10):** `SessionCoordinator` inserts the raw
  entry *before* any inference is attempted — in `submit` (foreground) and in
  `handleNotificationReply` (synchronously, before the background-task window
  is even requested). Inference failure only marks the turn "failed"; the row
  is already in SQLite.
- **Well-formed tool calls (§10):** GBNF grammar (`ToolGrammar.gbnf`)
  constrains sampling to the exact JSON schema of §5, so malformed output can't
  be generated. The parse layer is tolerant anyway, and `ToolExecutor` guards
  every action individually: unknown tool → silently dropped, > 3 actions →
  ignored, bad args → dropped. Covered by `ToolExecutorTests` /
  `ModelResponseTests`.
- **Stateless model, memory in SQLite (§1):** every turn clears the KV cache
  and receives a freshly assembled context block (`buildContext`, format per
  §6, ~1,500-token budget enforced by caps on recents/lengths plus a hard
  `promptTooLong` check).
- **Notification reply flow (§7):** insert → mark check-in responded → set
  `pending_turn_entry_id` → reschedule chain → best-effort inference inside
  `beginBackgroundTask`. If the window runs out, the pending marker survives
  and `processPendingTurn()` finishes the turn on next foreground.
- **Check-ins with no background process (§7):** a chain of the next 8
  non-repeating notifications is scheduled ahead; every app-open or reply
  cancels and reschedules from now. Quiet hours shift a slot to the window's
  end (midnight-wrapping handled).
- **RAM degradation (§9):** tier auto-selects 1.7B when `physicalMemory < 6 GB`;
  `os_proc_available_memory()` is checked for ~3 GB headroom before each load.
- **Thermals (§9):** inference happens only on user-initiated turns; no
  speculative generation; the sampler budget caps output at 320 tokens.

## Phase status (plan §8)

- **Phase 1 — skeleton + persistence: done.** The app is a usable journaling
  tool with zero AI: manual entry, timeline, task list, notification check-ins
  with text replies.
- **Phase 2 — inference: done** (code-complete; needs on-device benching for
  the <1.5 s TTFT / <6 s full-response targets and llama.cpp pin).
- **Phase 3 — voice: done** (TTS of `speech`, push-to-talk STT, voice picker).
- **Phase 4 — polish: done** (daily recap, export JSON/Markdown, quiet hours,
  Live Activity; thread nudging comes from the model seeing `[open threads]`
  in every context block).
- **Stretch (remote endpoint): not built**, per plan — explicitly post-v1.

## Privacy notes (App Store, plan §9)

- All journal data stays on device; disclose "Data Not Collected" honestly in
  privacy labels.
- Weights download post-install from Hugging Face (~1.1–2.5 GB); surfaced to
  the user with progress, resume, and checksum verification.
- Mic/speech permission strings are set in `project.yml` → Info.plist.
