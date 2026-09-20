# KuGou `/song/url` And `/user/vip/detail`

This file records what was measured about these two endpoints through the local `KuGouMusicApi` service with a real account's cookies, on 2026-09-20 and 2026-09-21. It covers the cases where the call succeeds at the HTTP level but the caller still did not get what it wanted: no playable link, a lower rendition than was asked for, or membership that reads as absent.

## `/song/url` Request Parameters

- `album_audio_id` **overrides** `hash`. When the two disagree, the returned link is for the song `album_audio_id` points at, and the response's own `fileName` field proves which song that was.
- `hash` is typically the file hash of that song's 128k rendition.
- `quality` values observed in real requests: `128`, `320`, `flac`, `high`. `high` is the parameter that maps to the hi-res tier; `super` and `multitrack` were also observed. `interface.d.ts` lists further values.

## `/song/url` Response: Three Business States

The top-level `status` field separates three response shapes that a caller otherwise tends to lump together.

### `status: 1` — a link was issued

Carries `url` (sometimes also `backupUrl`), `extName`, `fileSize`, `timeLength`, `fileName`, and `priv_status`.

### `status: 2` — an explicit refusal for the requested quality

Carries `priv_status: 0` and a `fail_process` array, and no `url`.

### `status: 0` — resource not found or invalid parameters

Carries `error_code`. Observed values: `35104` (Hash not found) and `20010` (parameter error). This shape is completely different from `status: 2`: it is not a refusal of a quality, it is a failure to identify the resource at all.

`priv_status` appears on both `status: 1` and `status: 2` responses, so the business state is read from the top-level `status`, not from `priv_status` alone.

## `fail_process` Is A Recorded Observation, Not A Criterion

`fail_process` values are **not stable**. Within one hour, on one account, the same refusal produced `["pkg","buy"]`, `["buy"]`, and `[]`. It can be recorded as what the upstream said on that call, but it cannot be used to decide *why* a link was refused: an empty array does not mean "no reason", and a change in the array does not mean the situation changed.

## A Successful `status` Does Not Mean The Requested Quality Was Obtained

The degradation is silent. Measured: `quality=high` on a song with no Hi-Res rendition returns the 128k mp3 with `status: 1`, and its `fileSize` and `timeLength` are identical to what `quality=128` returns for the same song. `super` behaves the same way; `multitrack` yields an `mkv`.

What the caller actually got is therefore identified by `extName` + `fileSize` + `timeLength` (and `fileName`), compared against another quality's response for the same song — not by the fact that `status` was `1`.

## Human Verification

Without cookies these endpoints are easily pushed into a human-verification wall: HTTP **502** with body `{ errcode: 20028, status: 0, error: "本次请求需要验证" }`. A 502 carrying that `errcode` is a policy response rather than a transport failure, and retrying does not clear it.

## `/user/vip/detail`

The 概念版 App reaches the same data as `kugouvip.kugou.com/v1/get_union_vip?busi_type=concept`.

`data.busi_vip` is an array of **parallel** entitlements. Each entry carries its own `is_vip`, `vip_begin_time`, `vip_end_time`, `product_type`, and `vip_limit_quota`.

### The top level can say "not a member" while an entry says otherwise

The top-level `is_vip` and `vip_type` are frequently `0` while an entry inside `data.busi_vip` reports `is_vip: 1`. Reading only the top level therefore reports "not a member" for an account that is one.

### Entitlements expire silently, and independently

After its expiry instant an entry flips to `is_vip: 0` — no error, no status change, and nothing else about the account looks wrong. The observable consequence is that `/song/url` starts refusing higher qualities while the account still appears healthy; conversely, once a valid entitlement is present, requests that were previously refused immediately start returning links again.

Each entitlement has its own independent start/end window, so different `product_type` entries can expire on different dates.

The signal is in the response body, not the HTTP status: a completely healthy-looking `200` can encode "no entitlement".

### Not verified

The mapping from `product_type` values (`svip`, `tvip`, ...) to the App's Chinese membership names has **not** been verified, and no table of it is given here. Which entry in `data.busi_vip` a given quality depends on was not isolated either; what was observed is only that a valid entitlement in the array and `/song/url` returning higher qualities move together.

## How To Re-verify

With an account's cookies, `curl` these two endpoints directly, request the same `hash` at `quality=128`, `320`, `flac`, and `high`, and compare `status`, `fail_process`, `extName`, and `fileSize` across the four responses. Then `curl` `/user/vip/detail` and read `data.busi_vip[]` entry by entry rather than reading the top level.
