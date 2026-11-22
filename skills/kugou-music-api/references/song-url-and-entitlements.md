# KuGou `/song/url`, `/user/vip/detail` And `/youth/union/vip`

These observations cover missing playable links, unexpected renditions, and membership fields in responses from the local `KuGouMusicApi` service.

## `/song/url` Request Parameters

- `album_audio_id` **overrides** `hash`. When the two disagree, the returned link is for the song `album_audio_id` points at, and the response's own `fileName` field proves which song that was.
- `hash` is typically the file hash of that song's 128k rendition.
- `quality` values observed in real requests: `128`, `320`, `flac`, `high`. `high` is the parameter that maps to the hi-res tier; `super` and `multitrack` were also observed. `interface.d.ts` lists further values.

## `/song/url` Response: Four Business States

The top-level `status` field separates four response shapes that a caller otherwise tends to lump together.

### `status: 1` — a link was issued

Carries `url` (sometimes also `backupUrl`), `extName`, `fileSize`, `timeLength`, `fileName`, and `priv_status`.

### `status: 2` — an explicit refusal for the requested quality

Carries `priv_status: 0` and a `fail_process` array, and no `url`.

### `status: 0` — resource not found or invalid parameters

Carries `error_code`. Observed values: `35104` (Hash not found) and `20010` (parameter error). This shape is completely different from `status: 2`: it is not a refusal of a quality, it is a failure to identify the resource at all.

### `status: 3` — the whole song's play behavior is refused

Carries `priv_status: 0` and, in the sample observed, `fail_process: []` (key present, array empty); no `url`, and **no** `error_code`.

It is **not** a statement about the requested quality, and that is what separates it from `status: 2`: the same song returns `3` at `128`, `320` and `flac` alike, and neither substituting another rendition's `hash` nor adding `album_audio_id` changes it.

The renditions exist, so `3` must not be read as "no such rendition": the by-hash audio-info route (`/audio`, upstream `kmr.service.kugou.com/v1/audio/audio`) and `/v2/get_res_privilege/lite` both list that song's 128 / 320 / flac entries — flac included, each with a byte size — while `/song/url` refuses to hand a link over.

`/v2/get_res_privilege/lite` (local route `/privilege/lite`) is what tells the two refusals apart. For every quality of a `status: 3` song the entry carries the literal

    "_msg": "Deny: the current behavior of the audio is forbidden."

whereas an entry for a song that returns `status: 1` carries `"_msg": ""`. Same account, same request shape; only the song changes. In that sample the denied entries also read `privilege: 5` with `old_cpy: 1`, and the playable ones `privilege: 10` with `old_cpy: 0` — but `_msg` is the field that said what happened.

The payload names no reason for the denial. One case observed was a studio recording the App itself only offers as a Live version, which is consistent with a rights or pulled-recording restriction; nothing in the response says which of the possible reasons it is.

`priv_status` appears on `status: 1`, `2` and `3` responses, so the business state is read from the top-level `status`, not from `priv_status` alone. Observed: `priv_status: 1` with `status: 1`, and `priv_status: 0` with both `status: 2` and `status: 3` — it does not separate the two kinds of refusal.

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

After expiry an entry flips to `is_vip: 0`. With only `tvip` still active, songs that required a quality privilege were refused at `320`, `flac`, and `high`. A song that was already free at a higher rendition kept returning its link. Previously refused requests can return links once the new entitlement becomes effective. [Observed daily upgrade propagation](claim-and-upgrade.md#proxy-caching-and-observed-propagation)

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

With an account's cookies, `curl` `/song/url` and `/user/vip/detail` directly, request the same `hash` at `quality=128`, `320`, `flac`, and `high`, and compare `status`, `fail_process`, `extName`, and `fileSize` across the four responses. Then `curl` `/user/vip/detail` and `/youth/union/vip` and read `data.busi_vip[]` entry by entry rather than reading the top level; the request-side differences above reproduce by comparing those two routes, then dropping `opt_product_types`, adding `product_type=svip`, and dropping `busi_type`. For a `status: 3`, add `/privilege/lite` (which posts to `/v2/get_res_privilege/lite`) with the same `hash` and read each quality entry's `_msg` and `privilege`, and check the by-hash audio-info route for the same song's rendition list and sizes; that pair is how you show the refusal is not a missing rendition. Vary one thing at a time — the song, the `quality`, the `hash`, `album_audio_id` — since `3` is per-song.
