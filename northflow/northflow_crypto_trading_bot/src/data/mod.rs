use crate::core::Candle;
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use csv::ReaderBuilder;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct CsvRow {
    timestamp: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

fn parse_timestamp(s: &str) -> Result<DateTime<Utc>> {
    // Try unix epoch (seconds)
    if let Ok(ts) = s.parse::<i64>() {
        if ts > 1_000_000_000_000 {
            // milliseconds
            return Ok(Utc.timestamp_millis_opt(ts).single().unwrap_or(Utc::now()));
        }
        return Ok(Utc.timestamp_opt(ts, 0).single().unwrap_or(Utc::now()));
    }
    // Try ISO 8601
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    // Try "YYYY-MM-DD HH:MM:SS"
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(Utc.from_utc_datetime(&ndt));
    }
    anyhow::bail!("Cannot parse timestamp: {}", s)
}

pub fn load_csv_ohlcv<P: AsRef<Path>>(path: P) -> Result<Vec<Candle>> {
    let path = path.as_ref();
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("Cannot open CSV: {}", path.display()))?;

    let mut candles = Vec::new();
    for result in rdr.deserialize::<CsvRow>() {
        let row = result.with_context(|| "Failed to parse CSV row")?;
        let timestamp = parse_timestamp(&row.timestamp)
            .with_context(|| format!("Bad timestamp: {}", row.timestamp))?;
        candles.push(Candle {
            timestamp,
            open: row.open,
            high: row.high,
            low: row.low,
            close: row.close,
            volume: row.volume,
        });
    }

    candles.sort_by_key(|c| c.timestamp);
    Ok(candles)
}
