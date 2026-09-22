# LX Music Custom Source Implementation Patterns

These patterns are distilled from the official documentation and public LX Music scripts. The goal is to reuse stable structure, not to copy any specific coding style.

## Pattern 1: Static Proxy
Use this when there is already a stable backend API and the script only needs to translate it into LX Music `musicUrl`, `lyric`, or `pic` behavior.

Common structure:

- Constants such as `DEV_ENABLE`, `API_URL`, `API_KEY`, and `MUSIC_QUALITY`.
- `httpFetch()` to wrap `request()` in a Promise.
- `normalizeJsonBody()` to handle `resp.body` being either a string or an object.
- `extractMusicUrl()` to turn backend payload variants into one final URL string.
- `handleGetMusicUrl()` to map `source`, a song identifier, and `quality` into backend requests.
- Optional `checkUpdate()` for update metadata.
- `musicSources` generated from `MUSIC_QUALITY` keys.
- `on(EVENT_NAMES.request, ...)` to dispatch by `action`.
- `send(EVENT_NAMES.inited, ...)` to register the sources.

Prefer this pattern first. It is the most readable, easiest to debug, and closest to the official minimal example.

Typical helpers for proxy-style scripts:

```js
const normalizeJsonBody = (resp) => {
  if (typeof resp.body === 'string') return JSON.parse(resp.body)
  return resp.body || {}
}

const extractMusicUrl = (data) => {
  const raw = data.data?.url ?? data.url
  if (Array.isArray(raw)) return raw.find(Boolean) || ''
  return typeof raw === 'string' ? raw : ''
}
```

Static proxy watch-outs:

- Check `resp.statusCode` before assuming the proxy call succeeded.
- Do not assume `resp.body` is always a JSON string.
- Do not assume the backend always returns `data.url`; top-level `url` and URL arrays are both common.
- Pass `options.body` as the object, never `JSON.stringify` it; see `references/contract.md` (Mobile Differences) for why a string body breaks on mobile. If the backend is consumed by both current and already-deployed old scripts, accept the JSON-string-wrapped shape too.
- If LX still cannot play while backend logs show `200`, inspect the script's parsing and final `Promise` return value before debugging the backend.
- Do not auto-upgrade audio URLs from `http` to `https` unless you verified the CDN host has a valid certificate for that hostname.
- **Forward the song's display fields upstream, not just its id.** If the backend logs, attributes, or displays requests per song, send `musicInfo.name` / `musicInfo.singer` / `musicInfo.albumName` alongside the identifier. The script is the **only** hop that has them: a `song/url`-style upstream response carries just the play URL, so a backend that accepted only the id can never recover the names afterwards without adding a second upstream lookup. This is a one-line change in the request body and the difference between a log that says `海阔天空 (Live) — Beyond、黄家驹` and one that says `#A0AEFE0`. See `contract.md` (`What The Script Actually Receives As musicInfo`) for the exact field names.
- **Never surface the backend's prose to the user. Map its machine code to your own fixed message.** The play bar shows a rejected `err.message` on one ellipsized, non-tooltipped line, and on the common failure path it is replaced entirely by the host's source-switch sentence — so the text you do control has to be short, front-loaded, and attributable (`contract.md`, `How The Host Renders A Rejection`). Branch on the envelope's stable code field, keep a `code -> short message` table in the script, and give every table a fallback entry so an unknown code still produces your text rather than the backend's `message`/diagnostic string.
- **Sign your runtime errors yourself.** The host prefixes init-time failures with `自定义源「{name}」初始化失败：` but adds no attribution to a runtime rejection. Apply a short fixed prefix in one helper (a single `fail()` that every rejection goes through) so it cannot be forgotten on a new branch, and keep it short enough to leave room for the actionable part.
- **Validate the returned URL in the script.** The host rejects a non-string, over-2048-character, or non-`/^https?:/` `musicUrl` with the literal, untranslated message `failed`. A local check turns that into your own readable message.
- **Set an explicit `request()` `timeout` well below the host's 20 s deadline.** `timeout` is in milliseconds and defaults to 60 000 when omitted, so the default never fires before the host rejects the request with its hardcoded English `Cancel request`.
- **Use the bare `服务器繁忙` signal only for server-declared transient failures, and only a bounded number of times.** It is compared by exact string equality, so it must be the message's entire content, and it routes into the host's own retry loop, which is not bounded by the already-retried guard. Borrow it to skip the source-switch fallback when a retry can genuinely succeed; never prefix or decorate it, and never return it indefinitely — after the cap, return your own readable message.

## Pattern 2: Dynamic Config
Use this when supported sources, qualities, switches, or update metadata come from a remote config instead of being hard-coded.

Common structure:

- Initialization first requests a config endpoint such as `latest`, `init.conf`, or a similar bootstrap API.
- The config response describes available sources, available qualities, version info, update notes, and update URLs.
- The local script uses `currentScriptInfo.version` or a digest of `currentScriptInfo.rawScript` to compare versions or validate integrity.
- After config is accepted, build `sources` dynamically and then send `inited`.

Watch-outs:

- If the remote config fails, decide whether the script should fail hard or use a local fallback.
- The more logic depends on remote config, the more fragile initialization becomes.
- If script hashing is needed, prefer host helpers such as `lx.utils.crypto.md5()` instead of pulling in extra code.

## Pattern 3: Signing Or Crypto Helpers
Use this when the upstream API requires signatures, encryption, compression, or binary conversion.

Common tools:

- `lx.utils.buffer` for binary and string conversion.
- `lx.utils.crypto` for `md5`, AES, RSA, or random bytes.
- `lx.utils.zlib` for compression or decompression.

Guidance:

- Only add these steps when the API actually requires them.
- Put signing logic in small helpers before attaching it to the main handlers.

## Pattern 4: Update Prompt
Public scripts often include this, but it is optional. See `contract.md` (`EVENT_NAMES.updateAlert`) for the host mechanics this section leans on.

Common approach:

- A constant flag such as `UPDATE_ENABLE`.
- An update check right **after** initialization, never before it.
- If the answer is "there is a newer version", call `send(EVENT_NAMES.updateAlert, { log, updateUrl })`.

Watch-outs:

- **Only versions imported after the check exists can ever be notified.** The check has to live inside the imported script, so the release that introduces it reaches nobody by itself — entries imported earlier stay unreachable through this channel forever. Ship it early rather than at the first change that genuinely needs announcing.
- **Send it after `inited`, fire-and-forget.** Anything thrown before `inited` can fail the import, and the check needs the network; awaiting it inside initialization buys nothing.
- **Let something outside the script decide whether there is an update.** A version baked into the script at render time always equals `currentScriptInfo.version`, and the host itself never compares versions. Ask the server that served the script, and let it own the comparison rule — including what to do when the version it serves is older than the one the user holds.
- **Validate the payload shape before sending.** The host spends its single opportunity before it validates, so one malformed or empty `log` costs the whole run.
- **Keep the check off the error path.** It must not share a helper that produces user-facing playback messages, must not borrow a retry signal, and must not touch any retry budget: an unreachable or failing update endpoint has to look exactly like "no update". Handle the `send()` rejection for the same reason.
- **Serve the notes from wherever the version comes from.** A script cannot carry notes for a release it is not, and notes fetched at check time can cover every version the user skipped — which is what makes a breaking change legible to someone jumping several releases.
- **Write the notice as short lines, not prose.** Newlines are the only formatting the update alert honours, and the dialog's width follows its longest line rather than a fixed frame, so one long sentence turns the popup into a full-window-wide wall while short itemized lines keep it a compact block. Put the thing the user must act on (an import URL, a command) on its own line, and keep each note short enough not to become the longest line.
- **The notice has to carry the import URL.** There is no in-place update and the host's online-import dialog consumes a URL, so "please update" without an address is not actionable. `log` is selectable plain text and is the reliable place for it; a clickable `updateUrl` is a bonus, not a substitute.
- Do not add update logic unless the task really needs it.

## Stable Cross-Script Observations
- Even obfuscated scripts still revolve around `globalThis.lx`, `EVENT_NAMES.request`, and `send(EVENT_NAMES.inited, ...)`.
- Many scripts use `musicInfo.hash ?? musicInfo.songmid` as a practical song-id fallback. That means API mapping code should be defensive about song identifiers.
- `env` and `version` are often used to build `User-Agent` or custom headers.
- Good scripts do not merely "normalize upstream response codes into a smaller set of script-level errors". The normalization exists so the *user-facing string* is chosen from a fixed code→message table and never inherited from the backend's prose. That string reaches one ellipsized line in the play bar, is frequently replaced outright by the host's own source-switch sentence, and is compared verbatim against the host's two reserved signals — so it must be short, front-loaded, self-attributable, and never a collision. See `contract.md` (`How The Host Renders A Rejection`) for the mechanics and the static-proxy watch-outs above for the rules.
- When maintaining obfuscated scripts, recover the event flow, initialization order, request shape, and return shape before worrying about the packing layer.

## Non-Standard Behavior To Treat Carefully
- Public examples may include `status: true`, extended `qualitys`, obfuscation shells, or extra metadata annotations. None of those automatically become part of the official contract.
- If the task is not specifically about compatibility with an existing script, do not introduce these extras by default.
- Only reuse extended qualities when the target LX version has already been verified to support them.

## Recommended Delivery Strategy
1. Start with the static proxy pattern by default.
2. Add remote config, signing, or update logic only when the task clearly needs them.
3. Every time initialization grows, re-check whether anything can throw before `inited` is sent.
4. Keep handler boundaries clear: `musicUrl`, `lyric`, and `pic` should stay separate.
