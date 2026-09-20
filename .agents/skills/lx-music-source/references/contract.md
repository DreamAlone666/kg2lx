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
- `musicInfo`: song metadata object. It is **not** just an id bag — see `What The Script Actually Receives` below.
- Return value: an HTTP or HTTPS song URL string.

#### What The Script Actually Receives As `musicInfo`
The object handed to the script is the **legacy flat shape**, not the host's internal one. The call site is
`musicSdk[musicInfo.source].getMusicUrl(toOldMusicInfo(musicInfo), itemQuality)` (`src/renderer/core/music/utils.ts`), and
`toOldMusicInfo()` (`src/common/utils/tools.ts`) flattens the nested `MusicInfo` before handing it over:

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

Provenance and evidence boundary: the official documentation only says `info.musicInfo` is 音乐信息对象，里面有音乐 ID、名字等信息 and **does not enumerate any field names**. The table above was verified by reading `lx-music-desktop` `master` (`src/common/utils/tools.ts` `toOldMusicInfo`, its call site in `src/renderer/core/music/utils.ts`, and the `LX.Music.MusicInfo*` types in `src/common/types/music.d.ts`). Treat it as source-verified but undocumented: stable in practice, yet not part of the published contract.

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

Everything in this section was read out of `lx-music-desktop` `master` (v2.12.6) and is **source-verified but undocumented**: the official documentation never describes what happens to a rejected `request()`. It is the difference between a script that reports something a user can act on and one that reports nothing at all.

### The Only Surface Is One Ellipsized Line
A rejection's `err.message` is the only text a script can put in front of the user:

- The last stop in the chain is `setAllStatus(err.message)` in `src/renderer/core/player/action.ts:138`; `setAllStatus` writes the same string into both `status` and `statusText` (`src/renderer/store/player/action.ts:53`).
- `statusText` is rendered as one `<div>` in every play-bar layout: `src/renderer/components/layout/PlayBar/MiniWidthProgress.vue:11`, `MiddleWidthProgress.vue:11`, `FullWidthProgress.vue:14`.
- That element is `.status { .mixin-ellipsis-1(); height: 23px }` (`MiniWidthProgress.vue:253`, `MiddleWidthProgress.vue:251`, `FullWidthProgress.vue:263`) inside a 13px `line-height: 1.5` box (`MiniWidthProgress.vue:241`). It carries **no `title` attribute and no tooltip** — unlike the song title two lines above it, which is selectable and click-to-copy.
- Consequence: text past the visible width is replaced by an ellipsis with no way to recover the full string. **Lead with the actionable words.** A message that opens with a script name, an error code, or a stacked-up sentence spends its whole visible budget on text the user cannot act on.

### The Message Is Usually Discarded Before Display
Where a rejection travels decides whether the script's own text survives:

1. `getMusicUrl` (`src/renderer/core/music/index.ts:22`) → `src/renderer/core/music/online.ts:58` → `handleGetOnlineMusicUrl` (`src/renderer/core/music/utils.ts:291`).
2. On failure, `handleGetOnlineMusicUrl` calls `onToggleSource()` (`:318`) and then `getOtherSource(musicInfo)` (`:320`) — a cross-platform search with a **15 s** deadline (`:56-59`).
3. If that search returns any candidate, the request is walked against them by `getOnlineOtherSourceMusicUrl` (`:237`). That loop **skips every candidate failing `assertApiSupport()`** (`:257`), and `assertApiSupport` is `source == 'local' || qualityList.value[source] != null` (`src/renderer/store/utils.ts:14`).
4. With no supported candidate left, the loop falls out and throws the host's own `toggle_source_failed` (`:265`) — the long sentence `换源失败，请尝试手动在搜索页指定其他来源搜索该歌曲播放` (`src/lang/zh-cn.json:719`).
5. The script's own error is rethrown only on the branch where the cross-platform search returned **nothing** (`:331`).

So a script declaring a single source key can almost never get its message displayed: the search still finds the song on other platforms, but step 3 discards every one of them, and the user reads the host's sentence instead.

Confirmed empirically **2026-09-20**: with the backend stopped and an uncached song, the play bar showed `换源失败，请尝试手动在搜索页指定其他来源搜索该歌曲播放`, not the script's `err.message`.

### Two Message Strings Are Compared With Exact Equality
The constants live in `src/renderer/utils/message.ts`: `cancelRequest: '取消http请求'` (`:7`) and `tooManyRequests: '服务器繁忙'` (`:8`). Both are compared against `err.message` with `==` (whole-string, no `includes`):

- `src/renderer/core/player/action.ts:118` — `err.message == requestMsg.cancelRequest` is treated as a **user cancellation**: the promise resolves `null`, no status is written, nothing is shown, no auto-skip.
- `src/renderer/core/player/action.ts:120` — `err.message == requestMsg.tooManyRequests` routes into `delayRetry` (`:67`), which waits `getRandom(2, 6)` seconds (`:70`) and re-requests.
- `src/renderer/core/music/utils.ts:317` and `:282` — the same busy string is rethrown **before** `onToggleSource()` and before the candidate loop, so it short-circuits the source-switch fallback described above.

Two rules follow:

- **A script's message must never collide with `'取消http请求'`**, or the failure is silently swallowed as if the user had cancelled.
- **`'服务器繁忙'` is a retry signal and must be the entire message.** Because the comparison is exact, any prefix or decoration breaks it. It is also checked *before* the already-retried guard: `delayRetry` re-enters `getMusicPlayUrl` with `isRetryed = true` (`:73`), and that flag is only consulted on the non-busy path (`:122`). A script that keeps answering `服务器繁忙` keeps the player in a 2–6 s retry loop indefinitely. Use it only for genuinely transient conditions, and only a bounded number of times; a permanent answer of `服务器繁忙` is a hang, not a message.

### The Host's Own Deadline Produces An Untranslated English String
`src/main/modules/userApi/rendererEvent/rendererEvent.ts:137` arms a 20 s timer for every script request; when it fires it calls `cancelRequest(requestKey)`, which rejects the pending request with the hardcoded literal `new Error('Cancel request')` (`:119`).

That string is **not** `requestMsg.cancelRequest` (`'取消http请求'`), so it matches neither the cancellation branch nor the busy branch. It falls through to `setAllStatus(err.message)` and the user reads an English `Cancel request` on the play bar — plus an auto-skip when `player.autoSkipOnError` is enabled (`action.ts:140`). A script should set its own `request()` timeout well below 20 s so the failure is reported in its own words first.

### `request()` Timeout Is In Milliseconds And Defaults To 60 s
`src/main/modules/userApi/renderer/preload.js:194` accepts `{ timeout }`; `:211` does `options.response_timeout = typeof timeout == 'number' && timeout > 0 ? Math.min(timeout, 60_000) : 60_000`. The value is **milliseconds**, clamped to 60 000, and **defaults to 60 000 when omitted** — longer than the host's own 20 s deadline, so relying on the default means the script's timeout never fires first.

### The `musicUrl` Return Value Is Validated With A Literal `failed`
`preload.js:78` (and `:96` for `pic`) rejects any `musicUrl` response that is not a string, exceeds 2048 characters, or fails `/^https?:/` — with the untranslated literal message `failed`. Validate the URL in the script so a user never reads `failed` on the play bar.

### Init Failures Get Attribution; Runtime Failures Do Not
A failure thrown before `inited` is shown in a dialog the host prefixes itself: `自定义源「{name}」初始化失败：` + the message, in full and selectable (`src/renderer/core/useApp/useInitUserApi.ts:136`, string at `src/lang/zh-cn.json:735`). A runtime rejection gets none of that — the play-bar line carries no source name at all. A script that wants its runtime errors attributable must sign them itself, with a short fixed prefix.

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

- Song lists, search, lyric, and cover art stay built-in. Verified for `kg` in `src/renderer/utils/musicSdk/kg/index.js`: `songList` / `musicSearch` / `hotSearch` / `pic` / `lyric` are direct local imports (`:3-7`, used at `:14-16`, `:22`, `:28`), while only `getMusicUrl` goes through the pluggable layer (`:18-19`, delegating to `apis('kg')`).
- `useInitUserApi.ts` builds `apis[source].getMusicUrl = (songInfo, type) => sendUserApiRequest({ source, action: 'musicUrl', info: { type, musicInfo: songInfo } })` for every declared action, and `src/renderer/utils/musicSdk/api-source.js:34` returns that user-api object when the active api source matches `/^user_api/`.

Two practical consequences:

- **Song lists, search, and therefore `musicInfo` come from the built-in source, not from you.** This is why `name` / `singer` / `albumName` are populated — you are not expected to supply them, and there is no action through which you could. (There is no list/search action at all: for a non-`local` source, `actions` is effectively fixed to `['musicUrl']`.)
- **Declaring a built-in key does not take over lyric or cover art.** Those still resolve through the built-in implementation. The rule "non-`local` sources expose only `musicUrl`" is about what you may declare, not about what the app can do for that source.

### There Is No Built-In `musicUrl` To Shadow
The pluggable layer is **empty** on a stock install, so there is nothing behind `apis(source)` for a custom source to fall back on:

- `src/renderer/utils/musicSdk/api-source-info.ts:32` default-exports a `sources` array whose only entries are commented out (`:9-29`), so it is empty.
- `src/main/modules/userApi/config/index.ts:2` is `export const userApis: LX.UserApi.UserApiInfoFull[] = []`.
- `src/renderer/utils/musicSdk/api-source.js:33-38`: `apis(source)` returns the user-api implementation only when the active api source matches `/^user_api/` (`:34`); otherwise it looks the source up in `apiList` — which is built from the empty `apiSourceInfo` (`:24-29`) — finds nothing, and throws `Api is not found` (`:37`).

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
