use crate::core::Candle;
use std::{fs, path::Path};

pub fn load_ohlcv_csv(path: impl AsRef<Path>) -> Result<Vec<Candle>, String> {
    let raw = fs::read_to_string(path.as_ref())
        .map_err(|e| format!("failed to read CSV {}: {e}", path.as_ref().display()))?;
    let mut lines = raw.lines();
    let Some(header) = lines.next() else { return Ok(Vec::new()) };
    let cols: Vec<String> = header.split(',').map(|s| s.trim().to_lowercase()).collect();
    let idx = |name: &str| cols.iter().position(|c| c == name);
    let ts_i = idx("timestamp").or_else(|| idx("open_time")).unwrap_or(0);
    let open_i = idx("open").unwrap_or(1);
    let high_i = idx("high").unwrap_or(2);
    let low_i = idx("low").unwrap_or(3);
    let close_i = idx("close").unwrap_or(4);
    let volume_i = idx("volume").unwrap_or(5);
    let mut candles = Vec::new();
    for (row_no, line) in lines.enumerate() {
        if line.trim().is_empty() { continue; }
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        let candle = Candle {
            timestamp: parse_i64(fields.get(ts_i).copied()).unwrap_or(row_no as i64),
            open: parse_f64(fields.get(open_i).copied())?,
            high: parse_f64(fields.get(high_i).copied())?,
            low: parse_f64(fields.get(low_i).copied())?,
            close: parse_f64(fields.get(close_i).copied())?,
            volume: parse_f64(fields.get(volume_i).copied()).unwrap_or(0.0),
        };
        if candle.is_valid() { candles.push(candle); }
    }
    Ok(candles)
}

fn parse_i64(value: Option<&str>) -> Option<i64> {
    let v = value?;
    v.parse::<i64>().ok().or_else(|| v.parse::<f64>().ok().map(|x| x as i64))
}

fn parse_f64(value: Option<&str>) -> Result<f64, String> {
    value.ok_or_else(|| "missing CSV field".to_string())?.parse::<f64>().map_err(|e| format!("invalid number in CSV: {e}"))
}
