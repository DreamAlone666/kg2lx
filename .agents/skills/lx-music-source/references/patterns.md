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
- For backends consumed by lx-music-mobile, accept both direct JSON object request bodies and JSON-string-wrapped object bodies. Mobile can double-serialize `options.body` when the script already used `JSON.stringify(payload)`.
- If LX still cannot play while backend logs show `200`, inspect the script's parsing and final `Promise` return value before debugging the backend.
- Do not auto-upgrade audio URLs from `http` to `https` unless you verified the CDN host has a valid certificate for that hostname.

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
Public scripts often include this, but it is optional.

Common approach:

- A constant flag such as `UPDATE_ENABLE`.
- An update-check request during or right after initialization.
- If the remote version is newer, call `send(EVENT_NAMES.updateAlert, { log, updateUrl })`.

Watch-outs:

- `updateAlert` can only be sent once per run.
- Do not add update logic unless the task really needs it.

## Stable Cross-Script Observations
- Even obfuscated scripts still revolve around `globalThis.lx`, `EVENT_NAMES.request`, and `send(EVENT_NAMES.inited, ...)`.
- Many scripts use `musicInfo.hash ?? musicInfo.songmid` as a practical song-id fallback. That means API mapping code should be defensive about song identifiers.
- `env` and `version` are often used to build `User-Agent` or custom headers.
- Good scripts normalize upstream response codes into a smaller set of script-level errors before rejecting.
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
