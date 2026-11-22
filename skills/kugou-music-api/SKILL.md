---
name: kugou-music-api
description: >-
  Help users call the local KuGouMusicApi (酷狗 API) and interpret its responses.
  Use for route selection, HTTP parameters, cookies or tokens, and curl, fetch,
  or Axios examples for search, lyrics, playback, playlists, users, albums, ranks,
  videos, radio, and QR or WeChat login. Also use for missing links, unexpected
  audio quality, human verification responses, and Concept Edition (概念版) VIP
  status, daily claims, upgrades, or record checks. Typical questions include
  “这个接口怎么调”, “为什么只有 128k”, “为什么会员显示过期”, and “今日 VIP 已领但未升级”.
---

# KuGou Music API

This skill is an API usage manual for the local `KuGouMusicApi` repository.

Optimize for one outcome: get the user to a working request quickly. This is not a repo tour skill, not an implementation tracing skill, and not a backend architecture explainer unless the user explicitly asks for that.

## Core stance

- Treat the repository as an API surface first and an implementation second.
- Prefer the smallest, highest-signal source that can answer the question.
- Stop reading as soon as the calling contract is clear.
- Focus on how to call the API, not how the server is built.

## Read order

For playback refusals, actual audio quality, or membership fields, start with [Song URLs and entitlements](references/song-url-and-entitlements.md). For daily claims, upgrades, records, or ad rewards, start with [Daily VIP operations](references/claim-and-upgrade.md).

Use this order and stop early if the answer is already clear:

1. `references/quick-reference.md`
2. `KuGouMusicApi/docs/README.md`
3. `KuGouMusicApi/interface.d.ts`
4. `KuGouMusicApi/README.md`
5. Minimal code fallback only when a real calling detail is still unresolved

## Search discipline

- Start with the bundled quick reference to avoid opening the large docs file for common questions.
- When you need the repo docs, grep for the route path or business term first, then read only the matching section.
- Search by endpoint path when possible, such as `/song/url`, `/search/lyric`, `/login/token`, or `/playlist/detail`.
- If the user states a goal instead of a route, map the goal to the smallest likely endpoint set first, then verify only those sections.
- For raw HTTP examples, prefer the parameter names shown in `KuGouMusicApi/docs/README.md` and the docs' URL examples.
- Treat `interface.d.ts` as a helper for enums, optionality, and type-like constraints, not as the final authority for external HTTP query names when docs and typings disagree.
- Use `README.md` for runtime details such as default host and port, proxy settings, and `platform=lite` notes.
- Do not widen into unrelated codebase exploration when the docs already answer the request.

## Conflict handling

- Separate external API parameters from upstream or internal field names.
- Treat response payload fields the same way: docs and typings may describe the route correctly while the local service returns a different field shape at runtime.
- If docs and typings conflict, prefer the documented route example first, then minimally verify the exact external parameter in code only if that difference would make the request fail.
- Do not surface an internal normalized field name to the user as if it were the public HTTP parameter.
- If you code-verify a conflict, state the final result plainly and keep the explanation to one line.

Current verified examples you should remember:

- In this repo, `酷狗概念版` means `platform=lite`.
- The external search routes use `keywords` in HTTP requests, even though internal request mapping converts that to upstream `keyword`.
- `/playlist/track/all/new` uses external `listid`; `lisdid` in `interface.d.ts` is stale for HTTP callers.
- QR login routes are especially cache-sensitive; use a unique `timestamp` on `/login/qr/key`, `/login/qr/create`, and `/login/qr/check` when freshness matters.
- `/login/qr/key` may return `data.qrcode` and `data.qrcode_img`, not just `data.key`.
- `/login/qr/create` may return `data.url` and `data.base64`, not just `data.qrurl` and `data.qrimg`.
- `/login/qr/check` can return `userid` as either an integer or a string.
- `/song/url` can return playable links at the top-level `url`, often as an array. The observed business states include link issued (`1`), requested quality refused (`2`), invalid or missing resource (`0`), and song-level playback refused (`3`). [Song URLs and entitlements](references/song-url-and-entitlements.md)
- `/user/vip/detail` can report Concept membership inside `data.busi_vip[]` even when top-level `is_vip` and `vip_type` are both `0`; that array holds several independent entitlements, each with its own start/end window. `/youth/union/vip` reaches the same upstream call with `opt_product_types` added, and returns the four entries where the default route returns two, the two it omits being the 1-day ones — see `references/song-url-and-entitlements.md`.

## When code fallback is allowed

Only fall back to source when documentation still does not settle one of these:

- exact route path or method
- exact parameter spelling
- cookie, token, or header placement
- auth requirement
- base URL or route prefix ambiguity
- a direct conflict between docs and typings
- a runtime response-shape mismatch that is causing parse failures or bad integration assumptions

When fallback is necessary:

1. Grep for the exact route path, parameter name, or module name.
2. Read only the matching file or the smallest relevant section, usually a matching file under `KuGouMusicApi/module/`.
3. Confirm only the blocked calling detail.
4. Report the answer as `Documented + code-verified` or `Implementation-inferred`.

Do not trace business logic, encryption internals, upstream KuGou calls, or other server internals unless the user explicitly asks for them.

## Answer shape

Mirror the user's language. Keep prose tight.

Use this structure when it helps:

### Call summary

- Purpose
- Method and path
- Auth or cookie requirement
- Required params
- Optional params

### Working example

- Default to `curl`.
- Add `fetch` or Axios only if the user asked or it clearly helps.
- Use concrete placeholder values so the user can see where each value goes.
- If auth matters, show exactly where the cookie or token belongs.
- Prefer a minimal copy-pasteable example over a broad option list.
- Reuse the high-frequency patterns from `references/quick-reference.md` before assembling a request from scratch.

### Notes and gotchas

- Include only invocation-relevant caveats.
- Mention cache, `timestamp`, `register/dev`, cross-origin credentials, login risk-control, and lite-platform caveats only when relevant to the route.
- If the user is debugging a runtime failure, mention verified response-shape gotchas only when they directly explain the failure.

### Source basis

End with one short line:

- `Documented`
- `Documented + code-verified`
- `Implementation-inferred`

If a detail still cannot be verified, say that plainly instead of guessing.

## Defaults for common requests

If the user wants search:

- Start with `/search` or `/search/complex`.
- Mention `keywords`, `type`, `page`, and `pagesize` when relevant.
- Do not rewrite the user-facing query parameter to `keyword`.
- If the user is debugging `error_code: 152` or `Parameter Error` on `/search`, do not assume the docs example still works unchanged; verify the current route behavior against the implementation before giving a confident fix.

If the user wants lyrics:

- Use `/search/lyric` first.
- Then use `/lyric` with the returned `id` and `accesskey`.

If the user wants a playable song URL:

- Recommend `/register/dev` first.
- Then use `/song/url`.
- Mention `/song/url/new` only when the user specifically wants the newer route or multiple quality URLs, and note the documented encrypted-audio caveat.
- When no link comes back, try a lower quality for `status: 2`, inspect the song's `_msg` on `/privilege/lite` for `status: 3`, or check the resource and request parameters for `status: 0`.

If the user wants daily Concept Edition VIP:

- Read current entitlements with `/youth/union/vip` and check daily grant progress with `/youth/month/vip/record`. The claim, upgrade, and ad routes change account state. Use the step-specific business code and a fresh daily record to resolve ambiguous results. [Daily VIP operations](references/claim-and-upgrade.md)

If the user wants playlist data:

- Distinguish `/playlist/detail` with `ids` from `/playlist/track/all` with `id`.
- Use `listid` for `/playlist/track/all/new` in HTTP examples.

If the user wants login guidance:

- Prefer `/captcha/sent` + `/login/cellphone`, QR login, or WeChat login flows when they fit.
- Mention that username and password login is documented as more fragile and may require verification.
- Tell the user to preserve returned cookies or token for follow-up authenticated calls.

If the user asks about default mobile versus `platform=lite` or `酷狗概念版`:

- Do not reduce the answer to token incompatibility alone.
- Keep the answer caller-facing.
- Explain the difference in terms of token and session scope, concept-only route availability, and any documented behavior differences that affect whether the caller should switch platform.
- Clarify that for callers of this local service, most common local route paths and basic calling style stay the same unless the route is explicitly concept-only.
- Do not explain internal signing, appid selection, RSA keys, upstream payload fields, or other implementation details unless the user explicitly asks for internals.

## What not to do

- Do not turn the answer into a repository walkthrough.
- Do not summarize internal modules when the user only needs request syntax.
- Do not inspect large source files unless a specific calling detail is blocked.
- Do not invent auth rules, parameters, or response fields.
- Do not spend tokens on implementation details that do not help the user make the call.
- Treat the user as an API caller, not a repo maintainer, unless they explicitly change the scope.

## Success condition

The skill succeeded if the user can copy the example request, replace placeholder values, and call the correct API without needing a codebase tour.
