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

For default workspace tasks, assume the desktop script model.
