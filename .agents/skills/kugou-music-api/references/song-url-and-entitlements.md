# KuGou `/song/url`, `/user/vip/detail` And `/youth/union/vip`

This file records what was measured about these endpoints through the local `KuGouMusicApi` service with a real account's cookies, on 2026-09-20 and 2026-09-21. It covers the cases where the call succeeds at the HTTP level but the caller still did not get what it wanted: no playable link, a lower rendition than was asked for, or membership that reads as absent.

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

## `/user/vip/detail` And `/youth/union/vip`

The 概念版 App reaches the same data as `kugouvip.kugou.com/v1/get_union_vip?busi_type=concept`.

`data.busi_vip` is an array of **parallel** entitlements. Each entry carries its own `is_vip`, `vip_begin_time`, `vip_end_time`, `product_type`, and `vip_limit_quota`.

### The request decides how many entries come back

That one upstream URL is reachable through two service routes, and they do not send the same parameters:

- `/user/vip/detail` sends `busi_type=concept` only, and returns **2** entries.
- `/youth/union/vip` additionally sends `opt_product_types=dvip,qvip`, and returns **4**.

The two entries the default route omits are the 1-day ones, so how many entitlements the account appears to hold is a property of the request, not of the account.

`busi_type` is required rather than optional: without it the call returns HTTP **502** with `error_code: 20010` (`request params invalid`). A 502 is therefore not by itself evidence of the verification wall described above — the body is what separates the two cases.

`product_type=svip` has **no effect** on the returned set: the set was unchanged across the four parameter combinations tested.

### The top level can say "not a member" while an entry says otherwise

The top-level `is_vip` and `vip_type` are frequently `0` while an entry inside `data.busi_vip` reports `is_vip: 1`. Reading only the top level therefore reports "not a member" for an account that is one.

### Entitlements expire silently, and independently

After its expiry instant an entry flips to `is_vip: 0` — no error, no status change, and nothing else about the account looks wrong. The consequence reaches `/song/url` **per song**, not per endpoint: measured with `tvip` as the only entry still in window, songs that need a quality privilege were refused at `320`/`flac`/`high`, while a song that is already free at a higher rendition kept returning its link. Conversely, once a valid entitlement is present, requests that were previously refused immediately start returning links again.

Each entitlement has its own independent start/end window, so different `product_type` entries can expire on different dates. Which entry grants access to which quality was not isolated beyond that `tvip`-only case.

The signal is in the response body, not the HTTP status: a completely healthy-looking `200` can encode "no entitlement".

### The Response Carries No Membership Name

A `data.busi_vip[]` entry is ids plus window and quota fields — `product_type`, `is_vip`, `vip_begin_time`, `vip_end_time`, `vip_limit_quota` among them. There is no name, title, or description field anywhere in the response. The Chinese membership names the App displays are client-side strings that the upstream does not return, so no field can be read to obtain them.

What has been matched with the App's own membership centre, by lining up an entry's `vip_end_time` against the date the App shows for that plan:

- `svip` — 概念版VIP
- `tvip` — 畅听VIP
- `qvip`, `dvip` — never matched to a name

The dates read back are that account's own entitlement windows, so they are not recorded here; compare both sides again to re-derive them (see How To Re-verify).

### The Time Fields

`vip_begin_time` and `vip_end_time` are formatted `YYYY-MM-DD HH:MM:SS`, carry **no timezone**, and can be an empty string. Each value is an instant, whereas the App displays a date.

The end value is not a constant. Two reads about 1.5 hours apart returned a different date for the same entry — the clock time unchanged, the date advanced by one day — while another entry read in the second call was unchanged. A window taken from one response is therefore a snapshot of that read, and a value carried over from an earlier read can be off by a day.

## How To Re-verify

With an account's cookies, `curl` `/song/url` and `/user/vip/detail` directly, request the same `hash` at `quality=128`, `320`, `flac`, and `high`, and compare `status`, `fail_process`, `extName`, and `fileSize` across the four responses. Then `curl` `/user/vip/detail` and `/youth/union/vip` and read `data.busi_vip[]` entry by entry rather than reading the top level; the request-side differences above reproduce by comparing those two routes, then dropping `opt_product_types`, adding `product_type=svip`, and dropping `busi_type`.
