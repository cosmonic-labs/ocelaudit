//! First-boot environment bootstrap + scheduled CSL corpus updates.
//!
//! The csl-service is the workload's long-running piece, so it owns "cron":
//!   1. On boot it stages the default environment into `/data` (the embedded SPA
//!      bundle from build.rs, minus videos) and seeds the corpus from an embedded
//!      fixture if empty, so the app is usable immediately — even offline.
//!   2. It then fetches the live CSL from data.trade.gov on start and once a day
//!      at 07:00 US Eastern, rebuilding + persisting the search index in place.
//!
//! No CronTrigger / extra component / extra workload needed; the cadence lives
//! here on `wasi:clocks`. Live fetch needs the service's `allowedHosts` to permit
//! data.trade.gov (see deploy/control/httptrigger.yaml).
use std::cell::RefCell;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use ocelaudit_search::SearchEngine;
use ocelaudit_storage_jsonfs::JsonFsStorage;

use crate::INDEX_CACHE_PATH;

// SPA bundle (ui/dist minus video/large media), embedded by build.rs.
include!(concat!(env!("OUT_DIR"), "/embedded_static.rs"));
// Offline fallback corpus (trade.gov "results" shape; parsed by csl-ingest).
const EMBEDDED_SEED: &[u8] = include_bytes!("../../../tests/fixtures/csl/sample.json");

const STATIC_ROOT: &str = "/data/static";
const TRADE_GOV_URL: &str =
    "https://data.trade.gov/downloadable_consolidated_screening_list/v1/consolidated.json";

/// Stage the default environment on first boot. Idempotent + best-effort.
pub fn stage_environment(storage: &JsonFsStorage) {
    if !Path::new(STATIC_ROOT).join("index.html").exists() {
        let mut staged = 0usize;
        for (rel, bytes) in EMBEDDED_STATIC {
            let dest = Path::new(STATIC_ROOT).join(rel);
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&dest, bytes).is_ok() {
                staged += 1;
            }
        }
        eprintln!("csl-service: staged {staged} SPA asset(s) into {STATIC_ROOT}");
    }
    // Seed only if empty, so there's a usable corpus before the live fetch lands.
    if storage.csl_list_all().map(|e| e.is_empty()).unwrap_or(true) {
        match ocelaudit_csl_ingest::parse_external_json(EMBEDDED_SEED) {
            Ok(entries) if !entries.is_empty() => {
                let n = entries.len();
                let _ = storage.csl_bulk_replace(entries, now_secs(), "seed");
                eprintln!("csl-service: seeded {n} records from the embedded fixture");
            }
            _ => {}
        }
    }
}

/// Fetch live CSL → replace corpus → rebuild + persist index → swap engine.
pub async fn run_update(engine: &RefCell<SearchEngine>, storage: &JsonFsStorage) -> Result<usize> {
    let bytes = fetch_live().await.context("fetch data.trade.gov")?;
    let entries = ocelaudit_csl_ingest::parse_external_json(&bytes).context("parse CSL")?;
    let n = entries.len();
    storage
        .csl_bulk_replace(entries.clone(), now_secs(), "live")
        .context("write csl.json")?;
    let built = SearchEngine::build(entries);
    if let Ok(b) = built.serialize_to_bytes() {
        let _ = std::fs::write(INDEX_CACHE_PATH, &b);
    }
    *engine.borrow_mut() = built;
    eprintln!("csl-service: live CSL update applied · n={n}");
    Ok(n)
}

/// Update on start, then daily at 07:00 US Eastern (override with UPDATE_UTC_HOUR
/// for testing). Never returns; failures are logged and retried next tick.
pub async fn scheduler(engine: &RefCell<SearchEngine>, storage: &JsonFsStorage) {
    if let Err(e) = run_update(engine, storage).await {
        eprintln!("csl-service: startup CSL update failed ({e:#}); keeping seeded corpus");
    }
    loop {
        let secs = secs_until_next_run(now_secs());
        eprintln!("csl-service: next CSL update in {}h{:02}m", secs / 3600, (secs % 3600) / 60);
        wstd::task::sleep(wstd::time::Duration::from_secs(secs)).await;
        if let Err(e) = run_update(engine, storage).await {
            eprintln!("csl-service: scheduled CSL update failed ({e:#}); retrying tomorrow");
        }
    }
}

async fn fetch_live() -> Result<Vec<u8>> {
    use wstd::http::{Client, Request};
    let req = Request::get(TRADE_GOV_URL)
        .header("user-agent", "OcelAudit-demo")
        .header("accept", "application/json")
        .body(())
        .context("build request")?;
    let mut resp = Client::new()
        .send(req)
        .await
        .map_err(|e| anyhow::anyhow!("send: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("trade.gov returned HTTP {}", status.as_u16());
    }
    let bytes = resp
        .body_mut()
        .contents()
        .await
        .map_err(|e| anyhow::anyhow!("read body: {e}"))?
        .to_vec();
    Ok(bytes)
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------- 07:00 US Eastern scheduling (DST-aware) ----------

fn secs_until_next_run(now: u64) -> u64 {
    if let Ok(h) = std::env::var("UPDATE_UTC_HOUR") {
        if let Ok(hour) = h.parse::<i64>() {
            return secs_until_utc_hour(now as i64, hour);
        }
    }
    let off = eastern_offset_secs(now as i64);
    let local = now as i64 + off;
    let day = local.div_euclid(86400);
    let mut target = day * 86400 + 7 * 3600; // 07:00 Eastern (local)
    if target <= local {
        target += 86400;
    }
    (target - off - now as i64).max(60) as u64
}

fn secs_until_utc_hour(now: i64, hour: i64) -> u64 {
    let day = now.div_euclid(86400);
    let mut target = day * 86400 + hour.rem_euclid(24) * 3600;
    if target <= now {
        target += 86400;
    }
    (target - now).max(60) as u64
}

/// US Eastern offset (seconds): EDT (-4h) inside DST, else EST (-5h). DST window:
/// 2nd Sunday of March 07:00 UTC → 1st Sunday of November 06:00 UTC.
fn eastern_offset_secs(now: i64) -> i64 {
    let (y, _, _) = civil_from_days(now.div_euclid(86400));
    let dst_start = days_from_civil(y, 3, nth_sunday(y, 3, 2)) * 86400 + 7 * 3600;
    let dst_end = days_from_civil(y, 11, nth_sunday(y, 11, 1)) * 86400 + 6 * 3600;
    if now >= dst_start && now < dst_end { -4 * 3600 } else { -5 * 3600 }
}

/// Day-of-month of the `n`-th Sunday in (year, month).
fn nth_sunday(y: i64, m: u32, n: i64) -> u32 {
    let first_wd = (days_from_civil(y, m, 1) + 4).rem_euclid(7); // 1970-01-01 = Thu(4); Sun=0
    (1 + (0 - first_wd).rem_euclid(7) + (n - 1) * 7) as u32
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (if m > 2 { m - 3 } else { m + 9 }) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nth_sunday_known_dates() {
        // 2024: DST start = Sun Mar 10; DST end = Sun Nov 3.
        assert_eq!(nth_sunday(2024, 3, 2), 10);
        assert_eq!(nth_sunday(2024, 11, 1), 3);
        // 2025: DST start = Sun Mar 9; DST end = Sun Nov 2.
        assert_eq!(nth_sunday(2025, 3, 2), 9);
        assert_eq!(nth_sunday(2025, 11, 1), 2);
    }
    #[test]
    fn eastern_offset_dst_vs_standard() {
        // 2025-07-01 (summer) → EDT (-4h); 2025-01-01 (winter) → EST (-5h).
        assert_eq!(eastern_offset_secs(days_from_civil(2025, 7, 1) * 86400 + 12 * 3600), -4 * 3600);
        assert_eq!(eastern_offset_secs(days_from_civil(2025, 1, 1) * 86400 + 12 * 3600), -5 * 3600);
    }
    #[test]
    fn next_run_is_in_future_and_bounded() {
        let now = days_from_civil(2025, 6, 15) * 86400 + 20 * 3600; // 8pm UTC
        let s = secs_until_next_run(now as u64);
        assert!(s >= 60 && s <= 24 * 3600);
    }
}
