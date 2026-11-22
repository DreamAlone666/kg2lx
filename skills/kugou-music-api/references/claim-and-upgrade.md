# Claiming And Upgrading The Daily VIP

These observations come from the local `KuGouMusicApi` service in Concept Edition (`platform=lite`) mode.

## Routes

Use cookies from a Concept Edition session. The methods below are for calls to the local service.

| Local route | Upstream | Method | Notes |
|---|---|---|---|
| `/youth/day/vip?receive_day=YYYY-MM-DD` | `youth/v1/recharge/receive_vip_listen_song` | POST | sends `source_id=90139` |
| `/youth/day/vip/upgrade` | `youth/v1/listen_song/upgrade_vip_reward` | POST | sends `ad_type=1`, userid from the cookie |
| `/youth/month/vip/record` | `youth/v1/activity/get_month_vip_record` | GET | `latest_limit=100` |
| `/youth/vip` | `youth/v1/ad/play_report` | POST | Consumes one ad reward slot |

## What each call does

- The claim adds **24 hours to `tvip`**, writing that day's entry in `/youth/month/vip/record` with `vip_type: "tvip"`.
- The upgrade turns that day's grant into `svip`; the same record entry flips to `vip_type: "svip"`. The response is `{"status":1,"data":{"recharge_hours":24}}`.
- Each step extends its own product's window using **`end = max(now, end) + 24h`**.
  - If the entitlement is still active, `vip_begin_time` is unchanged and `vip_end_time` moves forward by exactly 24 h.
  - If it has expired, the window starts at the backend's processing instant (a few seconds after the call) and ends 24 h later.
- `/youth/vip` reports one ad play and awards **3 hours** to **both `tvip` and `svip`**, at most 8 times a day. The response carries `total: 8`, `done`, `remain`, `award_vip_hour: 3`, `remain_vip_hour`. The awarded hours appear in the windows only after a delay of several minutes.

## Business responses and recovery

- `131001` confirms that the claim already completed for that day. Use the record to decide whether to continue with the upgrade.
- `297002` confirms that the upgrade already completed for that day. Read `/youth/union/vip` after it settles to get the current expiry.
- Inspect the JSON business code even on HTTP 502. The pinned proxy maps `status: 0` or a nonzero `error_code` to 502. An explicit `20028` indicates human verification. A 502 without a conclusive business result leaves the write outcome uncertain. [Proxy status mapping](https://github.com/MakcRe/KuGouMusicApi/blob/45f31fefe97b0503c5ae4c5c4ea652ed2b81e2a4/util/request.js#L136-L149)
- After a timed-out or interrupted write, read a fresh daily record before replaying the step. A dated `tvip` record confirms the claim and a dated `svip` record confirms both steps. Keep an outcome pending while the response is unreadable, the date is outside the returned range, or the write may still become visible.

## Proxy caching and observed propagation

The pinned proxy caches HTTP 200 responses for two minutes using only the hostname and URL as the cache key. Send `X-Apicache-Bypass: 1` on the local proxy's authenticated entitlement requests to obtain a fresh result for the current session. A unique `timestamp` query parameter can give read-only checks a fresh key when that header is unavailable. [Proxy cache registration](https://github.com/MakcRe/KuGouMusicApi/blob/45f31fefe97b0503c5ae4c5c4ea652ed2b81e2a4/server.js#L134-L142) · [Cache key and bypass](https://github.com/MakcRe/KuGouMusicApi/blob/45f31fefe97b0503c5ae4c5c4ea652ed2b81e2a4/util/apicache.js#L586-L631)

Read today's grant progress with the cookie jar from the local login flow.

```bash
curl -b cookies.txt -H 'X-Apicache-Bypass: 1' \
  'http://localhost:3000/youth/month/vip/record'
```

In the observed path through the proxy, `/song/url` at the target quality still returned `status: 2` for about 120 seconds after an upgrade from expiry. `/youth/union/vip` first showed `is_vip: 1` at about the same time. Use fresh reads when measuring this interval because the local proxy cache can contribute to the observed delay. The daily record's visibility delay and actual response envelope remain open.

## `/youth/vip` Is A Write

Calling this route consumes a slot and awards three hours. Its response carries the `done` counter for the action just taken.

## Time Fields

`svip`'s `vip_begin_time` and `vip_end_time` matched the local clock in the observation. The API's `tvip` end time differed from the Concept app display by +8 hours. Compare time deltas within a single `product_type`. kg2lx assumes UTC+8 for scheduling, including unzoned `svip` timestamps. The provider's cutoff and the day assigned to an upgrade spanning midnight remain unverified. [kg2lx scheduling](../../../server/README.md#每日权益领升)

## How To Re-verify

For a read-only check, compare `/youth/union/vip` with `/youth/month/vip/record` using the same session and fresh reads. When making an intended claim or upgrade, capture before and after responses and record when the target-quality link and the matching `svip` row first reflect the change. To verify that an active window was extended, compare only that row's start and end values across the two snapshots.
