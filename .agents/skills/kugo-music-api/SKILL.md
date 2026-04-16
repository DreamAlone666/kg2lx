---
name: kugo-music-api
description: Help users call `KuGouMusicApi` / 酷狗 API from this workspace with minimal repo reading. Use this skill whenever the user asks how to call a KuGouMusicApi endpoint, which endpoint to use for a goal, how to log in, how to pass cookie or token, what query/body params are required, or wants a `curl`/`fetch`/Axios example for search, lyric, song URL, playlist, user, album, rank, video, radio, QR login, WeChat login, or related 酷狗接口. Also use it for Chinese prompts like “这个接口怎么调”, “要传什么参数”, “怎么拿歌曲 url”, “歌词怎么取”, “歌单详情调哪个接口”, or “这个请求为什么不通” when the question is about calling the repo's exposed API rather than analyzing backend internals. Prefer `references/quick-reference.md`, `KuGouMusicApi/docs/README.md`, `KuGouMusicApi/interface.d.ts`, and `KuGouMusicApi/README.md`; avoid source digging unless docs conflict or omit a calling detail that would block a real request.
---

# Kugo Music API

This skill is an API usage manual for the local `KuGouMusicApi` repository.

Optimize for one outcome: get the user to a working request quickly. This is not a repo tour skill, not an implementation tracing skill, and not a backend architecture explainer unless the user explicitly asks for that.

## Core stance

- Treat the repository as an API surface first and an implementation second.
- Prefer the smallest, highest-signal source that can answer the question.
- Stop reading as soon as the calling contract is clear.
- Focus on how to call the API, not how the server is built.

## Read order

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
- If docs and typings conflict, prefer the documented route example first, then minimally verify the exact external parameter in code only if that difference would make the request fail.
- Do not surface an internal normalized field name to the user as if it were the public HTTP parameter.
- If you code-verify a conflict, state the final result plainly and keep the explanation to one line.

Current verified examples you should remember:

- In this repo, `酷狗概念版` means `platform=lite`.
- The external search routes use `keywords` in HTTP requests, even though internal request mapping converts that to upstream `keyword`.
- `/playlist/track/all/new` uses external `listid`; `lisdid` in `interface.d.ts` is stale for HTTP callers.

## When code fallback is allowed

Only fall back to source when documentation still does not settle one of these:

- exact route path or method
- exact parameter spelling
- cookie, token, or header placement
- auth requirement
- base URL or route prefix ambiguity
- a direct conflict between docs and typings

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

If the user wants lyrics:

- Use `/search/lyric` first.
- Then use `/lyric` with the returned `id` and `accesskey`.

If the user wants a playable song URL:

- Recommend `/register/dev` first.
- Then use `/song/url`.
- Mention `/song/url/new` only when the user specifically wants the newer route or multiple quality URLs, and note the documented encrypted-audio caveat.

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
