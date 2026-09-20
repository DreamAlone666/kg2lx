# KuGouMusicApi Quick Reference

This file is a compact call-oriented index distilled from:

- `KuGouMusicApi/docs/README.md`
- `KuGouMusicApi/interface.d.ts`
- `KuGouMusicApi/README.md`

Use this file first. Open the larger repo docs only when you need endpoint-specific detail.

## Source priority

1. `KuGouMusicApi/docs/README.md` for endpoint docs and examples
2. `KuGouMusicApi/interface.d.ts` for enums, optional fields, and type hints
3. `KuGouMusicApi/README.md` for runtime setup, host and port, proxy, and platform notes

## Runtime basics

- Default local base URL: `http://localhost:3000`
- Override with `HOST` and `PORT`
- HTTP proxy support: `KUGOU_API_PROXY` or `node app.js --proxy=http://127.0.0.1:7890`
- `酷狗概念版` in this repo means `platform=lite`
- Lite or concept mode: copy `.env.example` to `.env`, then set `platform=lite`
- Tokens are not interchangeable across platform variants

## Platform differences

- For callers of this local service, most common local route paths and basic calling style stay the same across default mobile and `platform=lite`.
- Tokens and login state are platform-scoped and not interchangeable.
- Some routes are concept-only, including documented examples like `/top/card/youth`, `/youth/vip`, `/youth/day/vip`, `/youth/day/vip/upgrade`, `/youth/month/vip/record`, and `/youth/union/vip`.
- Docs also indicate platform-specific behavior differences, such as concept-version login support and fixes for some songs that previously could not be fetched in concept mode.
- Caller guidance: switch to `platform=lite` before logging in if you need concept-only capabilities, and do not reuse a non-lite login state as if it were concept-mode login.
- Do not bring internal implementation differences into normal answers unless they are required to explain a caller-visible behavior.

## Call-wide caveats

- Docs show GET examples, but the service supports both GET and POST
- For POST requests, add a unique `timestamp` in the URL so the request URL is not cached
- Identical URLs are cached for about 2 minutes
- QR login is especially sensitive to cache reuse; add a unique `timestamp` to `/login/qr/key`, `/login/qr/create`, and `/login/qr/check`
- Cross-origin calls should include credentials such as `withCredentials: true` or `credentials: 'include'`, or pass cookies manually
- Preserve cookies or token after login for authenticated endpoints
- Do not spam login endpoints; docs warn repeated login calls may trigger risk control
- Public third-party deployments are risky because they may expose credentials

## Public parameter rules

- For raw HTTP requests, prefer the parameter names shown in `docs/README.md` examples.
- Treat `interface.d.ts` as supportive, especially for enums and optional fields.
- Verified external search parameter: use `keywords`, not `keyword`.
- Verified external playlist parameter for `/playlist/track/all/new`: use `listid`, not `lisdid`.

## Fast route selection

- Send login code: `/captcha/sent`
- Mobile login: `/login/cellphone`
- QR login: `/login/qr/key` -> `/login/qr/create` -> `/login/qr/check`
- WeChat login: `/login/wx/create` -> `/login/wx/check` -> `/login/openplat`
- Refresh login: `/login/token`
- Get `dfid` for song URL routes: `/register/dev`
- Song search: `/search`
- Mixed search: `/search/complex`
- Lyric lookup: `/search/lyric` -> `/lyric`
- Song URL: `/song/url`
- New song URL route: `/song/url/new`
- Playlist detail: `/playlist/detail`
- Playlist songs: `/playlist/track/all`
- User detail: `/user/detail`

## Common endpoint notes

### Login and auth

- `/captcha/sent`: requires `mobile`
- `/login/cellphone`: requires `mobile` and `code`; optional `userid` for multi-account cases
- `/login`: requires `username` and `password`; docs say this flow may require extra verification and is not the preferred first recommendation
- `/login/qr/create`: requires `key`; optional `qrimg`
- `/login/qr/check`: requires `key`
- `/login/wx/check`: requires `uuid`; `timestamp` is recommended to avoid cache delay
- `/login/token`: accepts `token` and `userid`
- In this workspace, `/login/qr/key` has been observed returning `data.qrcode` and `data.qrcode_img` instead of only `data.key`
- In this workspace, `/login/qr/create` has been observed returning `data.url` and `data.base64` instead of `data.qrurl` and `data.qrimg`
- In this workspace, `/login/qr/check` has been observed returning `userid` as either an integer or a string

### Search and lyric

- `/search`: docs use `keywords`; optional `type`, `page`, `pagesize`
- `/search/complex`: docs use `keywords`; optional `page`, `pagesize`
- `/search/hot`: no required params documented
- `/search/default`: no required params documented
- `/search/suggest`: docs example uses `keywords`
- `/search/lyric`: use `keywords` or `hash`; optional `album_audio_id`, `man`
- `/lyric`: requires `id` and `accesskey`; optional `fmt` and `decode`

### Song URL

- `/song/url`: requires `hash`; optional `album_id`, `album_audio_id`, `free_part`, `quality`
- `/song/url/new`: requires `hash`; optional `album_audio_id`, `free_part`
- Both URL routes currently need `/register/dev` first, otherwise docs warn about `本次请求需要验证`
- `interface.d.ts` lists quality values including `128`, `320`, `flac`, `high`, `piano`, `acappella`, `subwoofer`, `ancient`, `surnay`, `dj`, `viper_atmos`, `viper_clear`, and `viper_tape`
- Docs warn `/song/url/new` may return encrypted audio that is not currently decodable
- In this workspace, `/song/url` has been observed returning playable links at top-level `url`, often as an array, not only under `data.url`

### Playlist and user

- `/user/detail`: authenticated user info
- `/user/vip/detail`: Concept membership may show up in `data.busi_vip[]` even when top-level `is_vip` and `vip_type` are `0`
- `/user/playlist`: supports `page` and `pagesize`
- `/user/follow`: authenticated follow list
- `/playlist/tags`: playlist categories
- `/top/playlist`: requires `category_id`; optional `withsong`, `withtag`
- `/playlist/detail`: requires `ids`
- `/playlist/track/all`: requires `id`; optional `page`, `pagesize`
- `/playlist/track/all/new`: code-verified external parameter is `listid`; `lisdid` in typings is stale for HTTP callers
- `/playlist/similar`: requires `ids`

## Copy-paste templates

### Send login code

```bash
curl "http://localhost:3000/captcha/sent?mobile=13800138000"
```

### Mobile login and save cookies

```bash
curl -c cookies.txt -b cookies.txt \
  "http://localhost:3000/login/cellphone?mobile=13800138000&code=123456"
```

### Search songs

```bash
curl "http://localhost:3000/search?keywords=%E6%B5%B7%E9%98%94%E5%A4%A9%E7%A9%BA&type=song&page=1&pagesize=30"
```

### Search lyric, then fetch lyric

```bash
curl "http://localhost:3000/search/lyric?keywords=%E6%B5%B7%E9%98%94%E5%A4%A9%E7%A9%BA"
curl "http://localhost:3000/lyric?id=<LYRIC_ID>&accesskey=<ACCESS_KEY>&fmt=lrc&decode=true"
```

### Get dfid, then fetch song URL

```bash
curl -c cookies.txt -b cookies.txt "http://localhost:3000/register/dev"
curl -c cookies.txt -b cookies.txt \
  "http://localhost:3000/song/url?hash=<HASH>&quality=320"
```

### Get playlist detail

```bash
curl "http://localhost:3000/playlist/detail?ids=collection_3_1863870844_4_0"
```

### Get all songs in a playlist

```bash
curl "http://localhost:3000/playlist/track/all?id=collection_3_1863870844_4_0&page=1&pagesize=30"
```

### Get all songs in a user or collected playlist using the new route

```bash
curl -c cookies.txt -b cookies.txt \
  "http://localhost:3000/playlist/track/all/new?listid=<LIST_ID>&page=1&pagesize=30&timestamp=1710000000000"
```

### Fetch authenticated user detail with saved cookies

```bash
curl -b cookies.txt "http://localhost:3000/user/detail"
```

### Cross-origin fetch with cookies

```js
const res = await fetch('http://localhost:3000/user/detail', {
  credentials: 'include',
});
const data = await res.json();
```

## Known mismatches and gaps

- Docs usually say `keywords`, while `interface.d.ts` often uses `keyword` for search-related functions. External HTTP callers should use `keywords`.
- Docs example for `/playlist/track/all/new` uses `listid`, while typings use `lisdid`. External HTTP callers should use `listid`.
- Some runtime response payloads drift from docs and typings, especially QR login, song URL, and Concept VIP status. When a user is debugging parse failures, verify the current runtime shape before giving field names.
- In this workspace, documented `/search` examples have been observed returning `error_code: 152` (`Parameter Error`). If the user is debugging that exact failure, do not assume the docs example is still sufficient without code verification.
- Response payload schemas are not formally specified
- Some endpoints are marked as test-only, concept-only, or currently limited
- When an answer depends on one of these mismatches, verify only that exact detail instead of reading broad source files
