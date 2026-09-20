---
name: lx-music-source
description: Understand, review, explain, and write LX Music / 洛雪音乐 custom source scripts in this workspace. Use this whenever the user mentions 洛雪音乐、自定义音源、音源脚本、洛雪音源, LX Music custom source, `globalThis.lx`, `EVENT_NAMES`, `musicUrl`, `lyric`, `pic`, `qualitys`, source registration, wants to port a music API into an LX Music script, or is debugging why LX still cannot play even though the backend returns `200` or a valid URL, even if they only provide a JavaScript file or casually say “洛雪音源”.
---

Use this skill to keep LX Music custom source work aligned with the host contract, common implementation patterns, and a minimal review checklist. Default to the desktop documentation unless the user explicitly asks about mobile-specific behavior.

## Classify The Task First
Put the request into one of these buckets and only read the minimum needed files:

- Write a new script: read `references/contract.md` first, then choose the simplest pattern from `references/patterns.md`.
- Fix an existing script: read the target script first, then validate it against `references/contract.md`.
- Explain or review a script: prioritize host-contract violations, return-shape issues, and wrong `actions` / `qualitys` declarations.
- Port a third-party music API: map `LX request -> external API -> LX return value` before writing code.

## Core Principles
- Satisfy the LX host contract before optimizing for external API details.
- Prefer readable, minimal, maintainable code. Do not imitate obfuscated or bundled scripts unless the user explicitly needs that format.
- For non-`local` sources, default to exposing only `musicUrl`.
- Declare only the qualities you can actually resolve. `qualitys` directly controls what LX offers in the UI.
- Every branch in `on(EVENT_NAMES.request, ...)` must return a `Promise`.
- Any exception before `send(EVENT_NAMES.inited, ...)` can make script import fail, so keep initialization conservative.
- Only call `updateAlert` when there is a real update flow, and only once per run.
- Treat the official docs as authoritative. If example scripts use extra qualities or fields, only reuse them when the current target version is known to support them.
- Normalize `request()` callback payloads defensively. `resp.body` may already be an object, not always a JSON string.
- Do not assume a backend `200` guarantees LX playback. Script-side parse mistakes and wrong return shapes can still make playback fail.
- Pass `request()` `body` as an object, not a `JSON.stringify`'d string, and tolerate the JSON-string-wrapped body shape in backends; see `references/contract.md` (Mobile Differences) for why.
- Header metadata is parsed and **frozen at import time**: the host keeps its own copy, so re-rendering a script server-side changes nothing until the user deletes the entry and re-imports. Keep `@name`/`@description`/`@version` single-line, and never let user-controlled text reach the header without stripping `*/`.
- `@version` is header metadata like any other field, and the host uses it for **display only**: the source list renders it as `v{value}` when the value starts with a digit (`UserApiModal.vue:10`, `SettingBasic.vue:273`), and the script receives it back as `currentScriptInfo.version` (`preload.js:320`). Nothing in the host compares versions or decides anything from one — the only update path is `updateAlert`, which the script must send itself and which the user can switch off (`rendererEvent.ts:66`).
- `sources[key].name` is the **platform** display name (like `酷我音乐` / `本地音乐`), a different level from the header `@name`. Never drive it from a per-instance or user-editable title; see `references/contract.md` (`sources[key].name` Is Platform-Level).
- `info.musicInfo` is the host's **flattened legacy shape** (`toOldMusicInfo`), not the nested internal `MusicInfo` — read `musicInfo.albumName`, never `musicInfo.meta.albumName`. It carries the song's display fields (`name` / `singer` / `albumName`) alongside the source-specific ids, and they are populated because lists and search come from the built-in implementation; see `references/contract.md` (`What The Script Actually Receives As musicInfo`).

## Implementation Flow
1. Decide which source keys are needed: only use `kw`, `kg`, `tx`, `wy`, `mg`, and `local`.
2. Define `sources`, plus `actions` and `qualitys` for each source.
3. Wrap `request(url, options, callback)` with a Promise-style `httpFetch` helper.
4. Write separate handlers per action, for example `handleGetMusicUrl`, `handleGetLyric`, and `handleGetPic`.
5. Normalize request parameters, `resp.statusCode`, response payloads, and upstream errors inside the handlers.
6. Route everything through `on(EVENT_NAMES.request, ...)`.
7. Only after successful initialization, call `send(EVENT_NAMES.inited, { openDevTools, sources })`.

## Mapping Rules
- `info.type` is the LX-requested quality. Only continue if that quality is actually declared for the source.
- Choose the song identifier from real script or API data. For `kg` that is `musicInfo.hash`; `musicInfo.songmid` and `musicInfo.id` are the other common ones, and no script ever sees `musicInfo.meta.*`.
- If the backend logs or attributes requests per song, pass the display fields (`musicInfo.name` / `musicInfo.singer` / `musicInfo.albumName`) in the same request body. They are free at this hop and unrecoverable after it; see `references/patterns.md` (Pattern 1).
- `musicUrl` must finally resolve to one HTTP or HTTPS URL string, even if the backend returns `{ url }`, `{ data: { url } }`, or an array of candidate URLs.
- `lyric` must return `{ lyric, tlyric, rlyric, lxlyric }`, with missing fields set to `null`.
- `pic` must return an HTTP or HTTPS URL string.
- If the upstream API exposes many status codes, normalize them into a smaller set of clear script-level errors before rejecting.

## Debugging Playback Failures
- If backend logs show `200` but LX still cannot play, inspect the script before blaming the backend.
- Check `resp.statusCode`, body normalization, quality mapping, and the exact value returned from the `musicUrl` Promise.
- Common proxy mistakes are `JSON.parse(resp.body)` when `resp.body` is already an object, assuming `data.url` when the backend returns top-level `url`, and forgetting to unwrap a URL array.
- If desktop works but mobile fails, first separate script loading from runtime execution: confirm `/s/*.js` is fetched, confirm `/api/.../music-url` is called, then inspect whether the POST body is an object or a JSON-string-wrapped object. A server-side `400`/`422` before business logs usually means request extraction failed, not upstream playback failed.
- Do not blindly rewrite `http` audio URLs to `https`. Verify that the CDN host actually has a valid certificate for that hostname first.
- If your own error text never appears, assume it was **replaced** before display, not lost. On the common failure path the host runs a cross-platform search, skips every candidate it cannot support, and shows its own `换源失败，请尝试手动在搜索页指定其他来源搜索该歌曲播放` instead of your `err.message`; your message only survives when that search returns nothing. The discriminator is whether the cross-platform search found any candidate — see `references/contract.md` (How The Host Renders A Rejection).
- Untranslated English on the play bar (`Cancel request`) means the host's 20 s deadline fired, not that the script chose to fail. The host rejects its own timeout with a hardcoded English literal that matches none of its localized message constants.
- If the play bar shows `服务器繁忙`, the host is in its own 2–6 s retry loop rather than reporting a failure. A script that answers `服务器繁忙` on every attempt keeps the player retrying forever.

## Pattern Selection
- If the task is just wrapping an existing backend endpoint, use the static proxy pattern from `references/patterns.md`.
- If the available sources, qualities, or update metadata come from a remote config, use the dynamic config pattern.
- If the API needs signing, encryption, compression, or binary conversion, prefer `globalThis.lx.utils` instead of assuming extra runtime capabilities.
- If the user provides an obfuscated script, recover the event flow, initialization sequence, input mapping, and output mapping first. Do not start with deobfuscation for its own sake.

## Review Checklist
Before finishing, verify at least these points:

- The header comment includes `@name`; other metadata fields are present only when useful.
- The header has at most one line per field, `@name` / `@description` / `@version` stay single-line, and no field value can contain `*/` (repeatedly strip it — removing one occurrence can splice a new one together).
- No hard validation rejects a title merely for exceeding the official advisory lengths (24 characters for `@name`, 36 for `@description`); those are suggestions, and over-length values still import fine.
- `sources[key].name` is a fixed platform name, not an instance title or anything user-customizable.
- Every branch of `on(EVENT_NAMES.request, handler)` returns a `Promise`.
- `sources` keys, `actions`, and `qualitys` match the official contract.
- Non-`local` sources do not incorrectly expose `lyric` or `pic`.
- `inited` is sent at the right time and initialization does not contain obvious pre-init failures.
- `request()` response handling works whether `resp.body` is a string or an object.
- Backend proxy handlers tolerate both direct JSON object bodies and JSON-string-wrapped bodies when targeting lx-music-mobile.
- Backend proxy handlers check `resp.statusCode` and normalize nested or array URL payloads into one final string.
- If the backend renders, logs, or attributes per song, the outgoing request body carries the song's display fields (`musicInfo.name` / `singer` / `albumName`), not just the identifier. An id-only body is not something the backend can enrich later without an extra upstream lookup.
- `musicInfo` field reads use the flat names (`albumName`, `singer`, `hash`, `albumAudioId`) and never `musicInfo.meta.*`; no source-specific id is assumed to exist on another source.
- Headers, auth, UA strings, timeouts, and logging exist because the task needs them, not because example scripts had them.
- Every user-facing failure string is a fixed, short, front-loaded, self-attributable message picked from a code→message table; the backend's own prose is never displayed, and every table has a fallback for unknown codes.
- No failure message can equal the host's reserved signals verbatim: a message must never be exactly `服务器繁忙` except as a deliberate retry signal, and never exactly `取消http请求`.
- The retry signal, if used, is bare (no prefix or decoration) and bounded by a retry cap; it is never returned indefinitely.
- `request()` calls carry an explicit `timeout` below the host's 20 s deadline, in milliseconds.
- The value returned from `musicUrl` is validated locally (a string, at most 2048 characters, matching `/^https?:/`) before it resolves.
- If the script uses `env`, `version`, `currentScriptInfo`, or `utils`, only read the fields that are actually needed.

## Output Expectations
- When writing code: produce the smallest viable patch and keep the script structure obvious.
- When explaining code: use the order `host contract -> request mapping -> response mapping -> initialization/update behavior`.
- When reviewing code: lead with contract violations, regressions, wrong return values, and missing test coverage, then give a short summary.

## References
- Host contract and return shapes: `references/contract.md`
- Common implementation patterns distilled from official docs and public scripts: `references/patterns.md`
- Example eval prompts: `evals/evals.json`
