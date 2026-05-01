# M14 — TCP service benchmark

Same machine, same 25,600-record live `data.trade.gov` corpus, same
`Sberbank` query, 10 sequential `POST /api/v1/search` calls per
configuration.

| Configuration | p50 | range | server-side breakdown |
|---|---|---|---|
| **M11**: in-process engine, RefCell cache (didn't survive across requests) | 5,036 ms | 4,876 – 5,136 ms | csl_list_all + 20× re-read in `build_hit_snapshot` + `engine.build` per call |
| **M12** (chunked snapshot fix): in-process engine, RefCell cache (still didn't survive), `build_hit_snapshot` reads cached engine instead of re-reading 31 MB JSON 20× | 1,180 ms | 1,153 – 1,201 ms | mostly `engine.build`: csl_list_all=171 ms · build=956 ms · search=1 ms |
| **M13**: postcard disk cache (`/data/search-index.bin`, 11 MB blob) — gateway reinstantiated per request, hot-loads from disk | 247 ms | 245 – 260 ms | from_disk=210 ms · search=1 ms · audit=10 ms |
| **M14**: long-running TCP service, line-delimited JSON over loopback | **5 ms** | 4 – 7 ms | service_rpc=3 ms · audit_log=5–10 ms |

Server-side timing log lines (preserved in `wash dev`'s stderr) make
this measurable post-hoc — `grep "search timing" .cache/wash-dev.log`
after any session.

## What changed in M14

- New `components/csl-service/` workload (`wasi:cli/run` export). Owns
  the parsed `Vec<CslEntry>` and prebuilt `SearchEngine` for the host's
  lifetime. Listens on `127.0.0.1:7878` with a line-delimited JSON
  protocol.
- `components/api-gateway/src/csl_client.rs` — raw `wasi:sockets` TCP
  client. Connect, send one JSON line, read one JSON line, close.
- `/api/v1/{search, search/autocomplete, csl/stats, csl/refresh}` all
  forward to the service. The gateway's old in-process `SearchEngine`
  cache is gone (`AppState.engine` deleted; `ensure_engine` removed).
- `/api/v1/csl/refresh` post-`bulk_replace` now sends `{"op":"refresh"}`
  to the service, which re-reads `csl.json` from disk and rebuilds in
  RAM.
- Service warm-boots from the `search-index.bin` postcard cache when
  available, falls back to building from `csl.json`. Cold cold-start
  (no cache) is ~1.1 s in the service; subsequent gateway restarts
  don't trigger a rebuild because the service stays alive.

## Why the gain is so big

WASI P2 reinstantiates the `wasi:http/incoming-handler` component
**per request**. Anything in `OnceCell` / `RefCell` / `thread_local!`
gets discarded between requests. M11 and M12 both paid the engine-build
cost on every search because there was no way to share state.

M13 worked around that by serializing the prebuilt engine to disk and
hot-loading on each request — better, but still a 210 ms postcard
deserialize per call.

M14 splits the architecture: a `wasi:cli/run` **service** holds state
naturally (long-lived process), and the per-request component just
makes a loopback TCP call. That eliminates both the rebuild AND the
deserialize from the request path.

## Rough cost split (M14)

A `/api/v1/search` round-trip on a warm service:

```
3 ms   service_rpc       (TCP connect+write+read+close, JSON parse)
5 ms   audit_log         (append to /data/audit.jsonl)
1 ms   HTTP framing + per-request component instantiation
```

Audit log writes are now the dominant cost. Could be amortized further
(buffered writes flushed on a timer in a future milestone) but ~10 ms
total is well below human-perceptible.

## Re-running the benchmark

```sh
make demo                         # one terminal
bash tools/demo-queries.sh        # another terminal — populates audit log
grep "search timing" .cache/wash-dev.log | tail -20
```

Numbers will vary (host load, corpus size on disk) but the pattern
holds: the M14 mean is single-digit ms.
