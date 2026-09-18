# LX Music Custom Source Host Contract

This reference is distilled from the official documentation. It has higher priority than non-standard behavior found in public example scripts.

## Header Comment
The script must start with a metadata comment block. At minimum, include `@name`. Common fields:

- `@name`: required, keep it short.
- `@description`: optional.
- `@version`: optional, but usually worth keeping.
- `@author`: optional.
- `@homepage`: optional.

Example:

```js
/*!
 * @name Example Source
 * @description Notes here
 * @version 1.0.0
 * @author you
 * @homepage https://example.com
 */
```

### How The Host Uses Header Metadata
What a script actually gets from its header is narrower than the list of fields above. Treat only these as load-bearing:

- The host parses the comment and exposes it as `currentScriptInfo`. Rely on `@name`, `@version`, and `@description` having a stable, user-visible meaning; `@author` and `@homepage` are informational and are not guaranteed to be surfaced anywhere.
- `@name` is the title shown in the custom-source list; `@description` is the line under it. Both must stay **single-line** — a raw newline inside the comment breaks the metadata parse.
- Metadata is read and frozen **at import time**. The host keeps its own copy of the script text, so editing the script on a server does not change what an already-imported source displays. A user only sees a new title, description, or version after deleting the entry and importing again.
- There is **no auto-update** and **no de-duplication** for custom sources: importing the same URL twice yields two entries. This is why `@version` is the user's only signal for whether re-importing is worth it — do not bump it for changes that are invisible from the client side.
- The length limits are **advisory, not enforced**. The official documentation says 建议不要过长, 24 个字符以内 for `@name` and 36 个字符以内 for `@description`. Exceeding them does not fail the import; it only makes the list entry harder to read. Do not add hard validation that rejects an over-length name.
- Because `@name` sits inside a block comment, a value containing `*/` closes the comment early and makes the whole script a syntax error on import. Strip `*/` (repeatedly — removing one occurrence can splice a new one together) before writing any user-controlled text into the header.

### `sources[key].name` Is Platform-Level, Not Instance-Level
`@name` and each `sources[key].name` are two different levels and must not be driven by the same string:

- `@name` — the **script/instance** title; exactly one per script file.
- `sources[key].name` — the **platform** display name; one per declared source key.

The official basic example makes this explicit: the header is `@name 测试音乐源` (script level), while inside `inited` you find `kw: { name: '酷我音乐' }` and `local: { name: '本地音乐' }` (platform level). One script can declare several platform sources but has only one header `@name`, so sharing a single string between the two is a latent bug that appears as soon as a second source key is added.

Concretely: keep `sources.kg.name` a fixed platform literal such as `'酷狗概念版'`, and never wire it to a per-instance title that a user or a server can rename.

## `globalThis.lx`
The host exposes the scripting API through `globalThis.lx`. Common fields:

- `version`: custom-source API version.
- `env`: current runtime environment. Desktop is `desktop`.
- `currentScriptInfo`: parsed header metadata plus the raw script.
- `EVENT_NAMES`: event-name constants.
- `on(eventName, handler)`: register host-driven events.
- `send(eventName, data)`: send events back to the host.
- `request(url, options, callback)`: HTTP requests without browser CORS limits.
- `utils`: helper namespaces such as `buffer`, `crypto`, and `zlib`.

## Event Contract

### `EVENT_NAMES.inited`
Must be sent after initialization finishes. Typical shape:

```js
send(EVENT_NAMES.inited, {
  openDevTools: false,
  sources: {
    kw: {
      name: 'KuWo',
      type: 'music',
      actions: ['musicUrl'],
      qualitys: ['128k', '320k'],
    },
  },
})
```

Notes:

- Any exception before this event is sent can make script import fail.
- Desktop supports `openDevTools`; mobile does not.
- The official docs require `sources` and optionally `openDevTools`. Do not blindly copy extra fields such as `status` from public scripts.

### `EVENT_NAMES.request`
The host triggers this when it needs script functionality. The handler must return a `Promise`.

Common payload fields:

- `source`: current source key.
- `action`: requested action.
- `info`: action-specific payload.

#### `musicUrl`
`info` shape: `{ type, musicInfo }`

- `type`: requested quality, for example `128k`, `320k`, `flac`, or `flac24bit`.
- `musicInfo`: song metadata object. Common useful fields are `hash`, `songmid`, and `id`.
- Return value: an HTTP or HTTPS song URL string.

#### `lyric`
`info` shape: `{ musicInfo }`

- Return value: `{ lyric, tlyric, rlyric, lxlyric }`
- Missing fields should be `null`.

#### `pic`
`info` shape: `{ musicInfo }`

- Return value: an HTTP or HTTPS image URL string.

### `EVENT_NAMES.updateAlert`
Used to show an update prompt. It can only be sent once per run.

```js
send(EVENT_NAMES.updateAlert, {
  log: 'Update notes',
  updateUrl: 'https://example.com/update',
})
```

## Official Source And Action Rules

### Supported Source Keys
- `kw`
- `kg`
- `tx`
- `wy`
- `mg`
- `local`

### Action Rules
- `local` may support `musicUrl`, `lyric`, and `pic`.
- Other sources should normally expose only `musicUrl`.

### `qualitys` Rules
The official documentation lists these standard values:

- `128k`
- `320k`
- `flac`
- `flac24bit`

Some public scripts declare extra values such as `hires`, `atmos`, `master`, `2000k`, or `4000k`. Use this rule set:

- Prefer the official standard values by default.
- Reuse extended qualities only when the current target version and target script are already known to support them.
- Do not declare qualities just to look more capable.

## Recommended `request()` Wrapper
The host `request()` API is callback-based. Wrap it first so the rest of the script can stay async-friendly:

```js
const httpFetch = (url, options = { method: 'GET' }) => new Promise((resolve, reject) => {
  request(url, options, (err, resp) => {
    if (err) return reject(err)
    resolve(resp)
  })
})
```

## Minimal Skeleton

```js
/*!
 * @name Example Source
 * @version 1.0.0
 */
const { EVENT_NAMES, request, on, send } = globalThis.lx

const httpFetch = (url, options = { method: 'GET' }) => new Promise((resolve, reject) => {
  request(url, options, (err, resp) => {
    if (err) return reject(err)
    resolve(resp)
  })
})

const sources = {
  kw: {
    name: 'kw',
    type: 'music',
    actions: ['musicUrl'],
    qualitys: ['128k', '320k'],
  },
}

const handleGetMusicUrl = async (source, musicInfo, quality) => {
  const resp = await httpFetch(`https://example.com/url?source=${source}&quality=${quality}`)
  if (!resp.body?.url) throw new Error('get music url failed')
  return resp.body.url
}

on(EVENT_NAMES.request, ({ action, source, info }) => {
  switch (action) {
    case 'musicUrl':
      return handleGetMusicUrl(source, info.musicInfo, info.type)
    default:
      return Promise.reject(new Error('action not support'))
  }
})

send(EVENT_NAMES.inited, { openDevTools: false, sources })
```

## Mobile Differences
Only handle these when the user explicitly asks about mobile:

- Mobile `inited` does not support `openDevTools`.
- Some `lx.utils` methods are unavailable on mobile.
- Mobile has stricter limits around host APIs and built-in object mutation.
- `request()` serializes `options.body` once. Mobile strictly follows — it JSON-encodes whatever you pass, so a `JSON.stringify`'d payload double-wraps as a JSON string (e.g. `"{\"hash\":\"...\",\"quality\":\"flac\"}"`). Desktop is lenient — it sends a string body raw, papering over the wrong type, so the same `JSON.stringify`'d payload arrives as a plain JSON object (`{ "hash": "...", "quality": "flac" }`). Always pass `body` as the object, never `JSON.stringify` it yourself; otherwise mobile double-wraps and object-expecting backends reject it with `400`/`422` before any business log. Verified by wire capture 2026-07-28 (four requests, both platforms).

For default workspace tasks, assume the desktop script model.
