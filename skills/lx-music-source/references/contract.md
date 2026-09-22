# LX Music Custom Source Host Contract

This reference is distilled from the official documentation. It has higher priority than non-standard behavior found in public example scripts.

## Header Comment
The script must start with a metadata comment block. At minimum, include `@name`. Common fields:

- `@name`: required, keep it short.
- `@description`: optional.
- `@version`: optional.
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
- There is **no auto-update** and **no de-duplication** for custom sources: importing the same URL twice yields two entries, and the host never compares versions, so an already-imported entry changes only when the user deletes it and imports again. `@version`, like `@name` and `@description`, is therefore a value only a human ever reads.
- The length limits are **advisory, not a rejection criterion**. The official documentation says 建议不要过长, 24 个字符以内 for `@name` and 36 个字符以内 for `@description`, and exceeding them does not fail the import. Nor is the parsed value left intact: for every field the host knows a limit for, an over-length value is truncated at that limit with `...` appended. So an over-length `@name` or `@description` still imports, but what the source list displays and what the script reads back through `currentScriptInfo` is the cut string, not the full header text; only `currentScriptInfo.rawScript` carries the unmodified script. Do not add hard validation that rejects an over-length name.
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
- `musicInfo`: song metadata object. It is **not** just an id bag — see `What The Script Actually Receives` below.
- Return value: an HTTP or HTTPS song URL string.

Three properties of this action are easy to misread:

- **`type` is the requested quality and a script cannot change it.** Whatever URL the handler returns is recorded and cached by the host under the **requested** `type`, even when that URL points at a lower quality than was asked for. In the host, `type` is an internal value: it is not rendered as a quality label on the playback path.
- **A script cannot tell whether this call is playback or a download.** `type` and `musicInfo` are the whole payload; there is no flag, and no different action name, to distinguish the two.
- **A resolved URL is cached per song id + requested quality, and a cache hit is reused without asking the script again** unless the host is refreshing (`isRefresh`). Combined with the first point, a lower-quality URL obtained for a requested rung stays in the cache under that rung's name.

#### What The Script Actually Receives As `musicInfo`
The object handed to the script is the **legacy flat shape**, not the host's internal one. The host calls a source's `getMusicUrl` with `toOldMusicInfo(musicInfo)`, and that transform flattens the nested `MusicInfo` before handing it over:

| Script sees (flat) | Meaning | Internal path it came from |
| --- | --- | --- |
| `name` | Song title | `MusicInfo.name` |
| `singer` | Artist | `MusicInfo.singer` |
| `albumName` | Album name | `meta.albumName` |
| `songmid` | Song id | `meta.songId` |
| `interval` | Duration, formatted like `03:55` | `MusicInfo.interval` |
| `img` | Cover URL (may be `''`) | `meta.picUrl ?? ''` |
| `albumId` | Album id (optional) | `meta.albumId` |
| `types` / `_types` | Per-quality metadata and sizes | `meta.qualitys` / `meta._qualitys` |
| `hash` (kg only) | Kugou song hash | `meta.hash` |
| `albumAudioId` (kg only) | Kugou 专辑内歌曲ID / MixSongID | `meta.albumAudioId` |
| `strMediaMid` / `albumMid` (tx only) | tx identifiers | `meta.strMediaMid` / `meta.albumMid` |
| `copyrightId`, `lrcUrl`, `mrcUrl`, `trcUrl` (mg only) | mg identifiers | the matching `meta.*` fields |

Consequences that are easy to get wrong:

- **Never read `musicInfo.meta.*` in a script.** The nested form is the host's internal representation; by the time the handler runs, everything has been flattened one level. `musicInfo.meta.albumName` is `undefined`, `musicInfo.albumName` is the album.
- **Identifier names are source-specific and not normalized.** `hash` + `albumAudioId` are kg only. There is **no snake_case `album_audio_id`** — writing `musicInfo.album_audio_id || musicInfo.albumAudioId` gives you a first branch that can never be true.
- **`singer` is one string, not an array.** Multiple artists arrive joined by `、` (for example `Beyond、黄家驹`). Do not call `.join()` on it or index it.
- **Do not assume the ids are the only thing worth forwarding.** `name` / `singer` / `albumName` are populated whenever the song came from a list or search result, and are free to pass on. A backend that only receives ids can never recover the names later — see the proxy watch-out in `patterns.md`.
- **`types` / `_types` are the host's `meta.qualitys` / `meta._qualitys`, and they are catalogue metadata.** They state which rungs the song's own metadata lists, plus their sizes — not what any backend can currently deliver. They are unrelated to any account's entitlements, so they do not shrink merely because "this account cannot obtain that quality".

Provenance and evidence boundary: the official documentation only says `info.musicInfo` is 音乐信息对象，里面有音乐 ID、名字等信息 and **does not enumerate any field names**. The table above was verified against the host implementation, not the docs. Treat it as source-verified but undocumented: stable in practice, yet not part of the published contract.

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

## How The Host Renders A Rejection

Everything in this section was read out of `lx-music-desktop` `master` (v2.12.6, commit `ad95d50`, read 2026-09-20) and is **source-verified but undocumented**: the official documentation never describes what happens to a rejected `request()`. It is the difference between a script that reports something a user can act on and one that reports nothing at all. To re-derive any of it, grep the host source for the symbols and literals named in this file — `setAllStatus`, `getOtherSource`, `assertApiSupport`, `delayRetry`, `requestMsg`, `toOldMusicInfo`, `INFO_NAMES`, `isRetryed`, `isRefresh`, `Cancel request`, `服务器繁忙`, `FLAC Hires` — a grep anchor survives line drift and file reorganisation; a position does not.

### The Only Surface Is One Ellipsized Line
A rejection's `err.message` is the only text a script can put in front of the user:

- The last stop in the chain is `setAllStatus(err.message)`, which writes the same string into both `status` and `statusText`.
- `statusText` is rendered as a single line in every play-bar layout.
- That line is one ellipsized line (`.mixin-ellipsis-1()`, a 23px box inside 13px `line-height: 1.5`). It carries **no `title` attribute and no tooltip** — unlike the song title two lines above it, which is selectable and click-to-copy.
- Consequence: text past the visible width is replaced by an ellipsis with no way to recover the full string. **Lead with the actionable words.** A message that opens with a script name, an error code, or a stacked-up sentence spends its whole visible budget on text the user cannot act on.

### The Message Is Usually Discarded Before Display
Where a rejection travels decides whether the script's own text survives:

1. `getMusicUrl` → `handleGetOnlineMusicUrl`.
2. On failure, `handleGetOnlineMusicUrl` calls `onToggleSource()` and then `getOtherSource(musicInfo)` — a cross-platform search with a **15 s** deadline.
3. If that search returns any candidate, the request is walked against them by `getOnlineOtherSourceMusicUrl`. That loop **skips every candidate failing `assertApiSupport()`**, and `assertApiSupport` is `source == 'local' || qualityList.value[source] != null`.
4. With no supported candidate left, the loop falls out and throws the host's own `toggle_source_failed` — the long sentence `换源失败，请尝试手动在搜索页指定其他来源搜索该歌曲播放`.
5. The script's own error is rethrown only on the branch where the cross-platform search returned **nothing**.

So a script declaring a single source key can almost never get its message displayed: the search still finds the song on other platforms, but step 3 discards every one of them, and the user reads the host's sentence instead.

Confirmed empirically **2026-09-20**: with the backend stopped and an uncached song, the play bar showed `换源失败，请尝试手动在搜索页指定其他来源搜索该歌曲播放`, not the script's `err.message`.

### Two Message Strings Are Compared With Exact Equality
The host's message constants are `cancelRequest: '取消http请求'` and `tooManyRequests: '服务器繁忙'`. Both are compared against `err.message` with `==` (whole-string, no `includes`):

- `err.message == requestMsg.cancelRequest` is treated as a **user cancellation**: the promise resolves `null`, no status is written, nothing is shown, no auto-skip.
- `err.message == requestMsg.tooManyRequests` routes into `delayRetry`, which waits `getRandom(2, 6)` seconds and re-requests.
- The same busy string is rethrown **before** `onToggleSource()` and before the candidate loop, so it short-circuits the source-switch fallback described above.

Two rules follow:

- **A script's message must never collide with `'取消http请求'`**, or the failure is silently swallowed as if the user had cancelled.
- **`'服务器繁忙'` is a retry signal and must be the entire message.** Because the comparison is exact, any prefix or decoration breaks it. It is also checked *before* the already-retried guard: `delayRetry` re-enters `getMusicPlayUrl` with `isRetryed = true`, and that flag is only consulted on the non-busy path. A script that keeps answering `服务器繁忙` keeps the player in a 2–6 s retry loop indefinitely. Use it only for genuinely transient conditions, and only a bounded number of times; a permanent answer of `服务器繁忙` is a hang, not a message.

### One Failure Usually Costs Two Requests
The URL-resolution flow re-runs itself once, unconditionally, for any failure that is not "user cancelled / stopped / switched song". That second pass sets `isRetryed`, and the flag is consulted only **after** the retry has already been attempted — so it does not prevent the second request, it only prevents a third.

The consequence is that an observer — a backend, a log — sees the same request twice, roughly a second apart, and the second one is the host's own doing rather than anything the script or the backend asked for. Counting rows in a log therefore cannot answer "did the backend retry?"; only the backend's own retry counter can.

### The Host's Own Deadline Produces An Untranslated English String
The host's script-event layer arms a 20 s timer for every script request; when it fires it calls `cancelRequest(requestKey)`, which rejects the pending request with the hardcoded literal `new Error('Cancel request')`.

That string is **not** `requestMsg.cancelRequest` (`'取消http请求'`), so it matches neither the cancellation branch nor the busy branch. It falls through to `setAllStatus(err.message)` and the user reads an English `Cancel request` on the play bar — plus an auto-skip when `player.autoSkipOnError` is enabled. A script should set its own `request()` timeout well below 20 s so the failure is reported in its own words first.

### `request()` Timeout Is In Milliseconds And Defaults To 60 s
`request()` accepts `{ timeout }` and normalizes it with `typeof timeout == 'number' && timeout > 0 ? Math.min(timeout, 60_000) : 60_000`. The value is **milliseconds**, clamped to 60 000, and **defaults to 60 000 when omitted** — longer than the host's own 20 s deadline, so relying on the default means the script's timeout never fires first.

### The `musicUrl` Return Value Is Validated With A Literal `failed`
The host rejects any `musicUrl` (and `pic`) response that is not a string, exceeds 2048 characters, or fails `/^https?:/` — with the untranslated literal message `failed`. Validate the URL in the script so a user never reads `failed` on the play bar.

### Init Failures Get Attribution; Runtime Failures Do Not
A failure thrown before `inited` is shown in a dialog the host prefixes itself: `自定义源「{name}」初始化失败：` + the message, in full and selectable. A runtime rejection gets none of that — the play-bar line carries no source name at all. A script that wants its runtime errors attributable must sign them itself, with a short fixed prefix.

## Official Source And Action Rules

### Supported Source Keys
- `kw`
- `kg`
- `tx`
- `wy`
- `mg`
- `local`

### These Keys Are Built-In For Lists, Search, Lyric And Cover Art
None of these keys are blank slots for list/search/lyric/pic. Each one already has a built-in implementation inside LX for those actions, and those still resolve through the host:

- Song lists, search, lyric, and cover art stay built-in. For `kg`, `songList` / `musicSearch` / `hotSearch` / `pic` / `lyric` are direct local imports of the built-in music SDK, while only `getMusicUrl` goes through the pluggable layer (delegating to `apis('kg')`).
- The user-api init builds `apis[source].getMusicUrl = (songInfo, type) => sendUserApiRequest({ source, action: 'musicUrl', info: { type, musicInfo: songInfo } })` for every declared action, and `apis(source)` returns that user-api object when the active api source matches `/^user_api/`.

Two practical consequences:

- **Song lists, search, and therefore `musicInfo` come from the built-in source, not from you.** This is why `name` / `singer` / `albumName` are populated — you are not expected to supply them, and there is no action through which you could. (There is no list/search action at all: for a non-`local` source, `actions` is effectively fixed to `['musicUrl']`.)
- **Declaring a built-in key does not take over lyric or cover art.** Those still resolve through the built-in implementation. The rule "non-`local` sources expose only `musicUrl`" is about what you may declare, not about what the app can do for that source.

### There Is No Built-In `musicUrl` To Shadow
The pluggable layer is **empty** on a stock install, so there is nothing behind `apis(source)` for a custom source to fall back on:

- The built-in API-source list default-exports a `sources` array whose only entries are commented out, so it is empty.
- The user-api config is `export const userApis: LX.UserApi.UserApiInfoFull[] = []`.
- `apis(source)` returns the user-api implementation only when the active api source matches `/^user_api/`; otherwise it looks the source up in `apiList` — which is built from that empty list — finds nothing, and throws `Api is not found`.

Consequence: **a single-source custom script cannot rely on the host having any other playable source.** There is no built-in `musicUrl` implementation to shadow, so once the script's `musicUrl` handler rejects, resolution necessarily falls through to the source-switch path described in `How The Host Renders A Rejection` — and the user sees the host's sentence, not the script's.

Indirectly confirmed by the **2026-09-20** test: with the backend stopped, the cross-platform search still returned matches, yet URL resolution could only end at the source-switch fallback, because no candidate could pass `assertApiSupport()`.

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

### How The Requested Quality Is Chosen
Starting from the requested rung, the host walks `flac24bit` → `flac` → `320k` and takes the first rung that is **both** present in the song's metadata **and** declared by the source's `qualitys`. If none of those three qualifies, it falls back to `128k`.

For a custom source declaring only `musicUrl`, search and lists are served by the host's built-in implementation, so the song's metadata is the host's rather than the script's. That metadata is **catalogue-level**: it describes what the catalogue holds for the song. It is unrelated to the account's entitlements, so it does not degrade merely because "this account cannot obtain that quality".

### Quality Surfaces On Download, Not On Playback
- The download dialog lists the intersection of the song's metadata qualities and the source's declared `qualitys`, and the download list shows the chosen rung's name — for example `FLAC Hires`.
- The playback path renders no quality anywhere.

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
