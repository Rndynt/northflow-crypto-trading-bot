//! Generate candlestick chart images for signal notifications.
//!
//! Uses `plotters` with built-in candlestick element and PNG output.

use crate::data::Side;
use plotters::element::CandleStick;
use plotters::prelude::*;
use std::io::Read;
use tracing::debug;

/// Candle data for chart rendering.
#[derive(Debug, Clone, Copy)]
pub struct ChartCandle {
    pub open_time: chrono::DateTime<chrono::Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl From<&crate::data::Candle> for ChartCandle {
    fn from(c: &crate::data::Candle) -> Self {
        Self {
            open_time: c.open_time,
            open: c.open,
            high: c.high,
            low: c.low,
            close: c.close,
            volume: c.volume,
        }
    }
}

/// Fetch recent klines from Binance Futures public API.
/// Tries fapi.binance.com first, falls back to data-api.binance.vision.
pub async fn fetch_klines(
    client: &reqwest::Client,
    symbol: &str,
    interval: &str,
    limit: u32,
) -> Result<Vec<ChartCandle>, String> {
    let primary_url = format!(
        "https://fapi.binance.com/fapi/v1/klines?symbol={}&interval={}&limit={}",
        symbol, interval, limit
    );
    let fallback_url = format!(
        "https://data-api.binance.vision/api/v3/klines?symbol={}&interval={}&limit={}",
        symbol, interval, limit
    );

    // Try primary, then fallback
    let raw: Vec<serde_json::Value> = match client.get(&primary_url).send().await {
        Ok(resp) if resp.status().is_success() => resp
            .json()
            .await
            .map_err(|e| format!("kline parse: {}", e))?,
        _ => {
            debug!(
                "chart: primary kline failed, trying fallback for {}",
                symbol
            );
            client
                .get(&fallback_url)
                .send()
                .await
                .map_err(|e| format!("kline fallback fetch: {}", e))?
                .json()
                .await
                .map_err(|e| format!("kline fallback parse: {}", e))?
        }
    };
    let mut candles = Vec::with_capacity(raw.len());
    for r in &raw {
        let arr = r.as_array().ok_or("kline: not array")?;
        if arr.len() < 6 {
            continue;
        }
        let ts_ms = arr[0].as_i64().unwrap_or(0);
        let open_time = chrono::DateTime::from_timestamp_millis(ts_ms).unwrap_or_default();
        candles.push(ChartCandle {
            open_time,
            open: arr[1].as_str().unwrap_or("0").parse().unwrap_or(0.0),
            high: arr[2].as_str().unwrap_or("0").parse().unwrap_or(0.0),
            low: arr[3].as_str().unwrap_or("0").parse().unwrap_or(0.0),
            close: arr[4].as_str().unwrap_or("0").parse().unwrap_or(0.0),
            volume: arr[5].as_str().unwrap_or("0").parse().unwrap_or(0.0),
        });
    }
    debug!("chart: fetched {} klines for {}", candles.len(), symbol);
    Ok(candles)
}

/// Generate a candlestick chart PNG with entry/TP/SL levels.
pub fn generate_signal_chart(
    symbol: &str,
    side: Side,
    entry: f64,
    sl: f64,
    tp: f64,
    candles: &[ChartCandle],
) -> Result<Vec<u8>, String> {
    if candles.is_empty() {
        return Err("no candle data".into());
    }

    let w = 900u32;
    let h = 520u32;
    let n = candles.len() as i32;

    let bg = RGBColor(18, 18, 30);
    let bull = RGBColor(38, 166, 91);
    let bear = RGBColor(231, 76, 60);
    let grid = RGBColor(35, 35, 55);
    let txt = RGBColor(190, 190, 210);
    let entry_c = RGBColor(52, 152, 219);
    let sl_c = RGBColor(231, 76, 60);
    let tp_c = RGBColor(46, 204, 113);
    let gold = RGBColor(255, 215, 0);

    let mut pmin = candles.iter().map(|c| c.low).fold(f64::MAX, f64::min);
    let mut pmax = candles.iter().map(|c| c.high).fold(f64::MIN, f64::max);
    for &p in &[entry, sl, tp] {
        if p > 0.0 {
            pmin = pmin.min(p);
            pmax = pmax.max(p);
        }
    }
    let pad = (pmax - pmin) * 0.10;
    pmin -= pad;
    pmax += pad;

    let tmp_path = format!("/tmp/chart_{}.png", symbol.replace('/', "_"));
    {
        let root = BitMapBackend::new(&tmp_path, (w, h)).into_drawing_area();
        root.fill(&bg).map_err(fmt_err)?;

        let (title_area, chart_area) = root.split_vertically(36);

        let slabel = if side == Side::Long {
            "📈 LONG"
        } else {
            "📉 SHORT"
        };
        title_area
            .draw(&Text::new(
                format!(
                    "{} {}  ·  Entry {:.2}  SL {:.2}  TP {:.2}",
                    symbol.replace("USDT", ""),
                    slabel,
                    entry,
                    sl,
                    tp
                ),
                (15, 8),
                ("sans-serif", 18).into_font().color(&txt),
            ))
            .map_err(fmt_err)?;

        let mut chart = ChartBuilder::on(&chart_area)
            .margin_left(60)
            .margin_right(65)
            .margin_top(5)
            .margin_bottom(22)
            .build_cartesian_2d(0i32..n, pmin..pmax)
            .map_err(fmt_err)?;

        chart
            .configure_mesh()
            .x_label_formatter(&|x: &i32| {
                let i = *x as usize;
                if i < candles.len() {
                    candles[i].open_time.format("%H:%M").to_string()
                } else {
                    String::new()
                }
            })
            .y_label_formatter(&|y: &f64| format!("{:.2}", y))
            .x_labels(8)
            .y_labels(8)
            .label_style(("sans-serif", 10).into_font().color(&txt))
            .light_line_style(grid)
            .bold_line_style(grid)
            .draw()
            .map_err(fmt_err)?;

        // Candlesticks
        chart
            .draw_series(candles.iter().enumerate().map(|(i, c)| {
                CandleStick::new(
                    i as i32,
                    c.open,
                    c.high,
                    c.low,
                    c.close,
                    bull.filled(),
                    bear.filled(),
                    3,
                )
            }))
            .map_err(fmt_err)?;

        // Volume bars
        let vol_max = candles.iter().map(|c| c.volume).fold(0.0f64, f64::max);
        let vol_h = (pmax - pmin) * 0.12;
        if vol_max > 0.0 {
            chart
                .draw_series(candles.iter().enumerate().map(|(i, c)| {
                    let vh = (c.volume / vol_max) * vol_h;
                    let vc = if c.close >= c.open {
                        bull.mix(0.2)
                    } else {
                        bear.mix(0.2)
                    };
                    Rectangle::new([(i as i32, pmin), (i as i32, pmin + vh)], vc.filled())
                }))
                .map_err(fmt_err)?;
        }

        // Entry line
        if entry > 0.0 {
            chart
                .draw_series(std::iter::once(PathElement::new(
                    vec![(0i32, entry), (n, entry)],
                    ShapeStyle {
                        color: entry_c.to_rgba(),
                        filled: false,
                        stroke_width: 2,
                    },
                )))
                .map_err(fmt_err)?;
            chart
                .draw_series(std::iter::once(Text::new(
                    format!("ENTRY {:.2}", entry),
                    (1, entry + (pmax - pmin) * 0.018),
                    ("sans-serif", 11).into_font().color(&entry_c),
                )))
                .map_err(fmt_err)?;
        }

        // SL line
        if sl > 0.0 {
            chart
                .draw_series(std::iter::once(PathElement::new(
                    vec![(0i32, sl), (n, sl)],
                    ShapeStyle {
                        color: sl_c.to_rgba(),
                        filled: false,
                        stroke_width: 2,
                    },
                )))
                .map_err(fmt_err)?;
            let label = format!("SL {:.2}", sl);
            let lx = n - (label.len() as i32 / 2).max(6);
            chart
                .draw_series(std::iter::once(Text::new(
                    label,
                    (lx, sl + (pmax - pmin) * 0.018),
                    ("sans-serif", 11).into_font().color(&sl_c),
                )))
                .map_err(fmt_err)?;
        }

        // TP line
        if tp > 0.0 {
            chart
                .draw_series(std::iter::once(PathElement::new(
                    vec![(0i32, tp), (n, tp)],
                    ShapeStyle {
                        color: tp_c.to_rgba(),
                        filled: false,
                        stroke_width: 2,
                    },
                )))
                .map_err(fmt_err)?;
            let label = format!("TP {:.2}", tp);
            let lx = n - (label.len() as i32 / 2).max(6);
            chart
                .draw_series(std::iter::once(Text::new(
                    label,
                    (lx, tp + (pmax - pmin) * 0.018),
                    ("sans-serif", 11).into_font().color(&tp_c),
                )))
                .map_err(fmt_err)?;
        }

        // R:R badge
        if entry > 0.0 && sl > 0.0 && tp > 0.0 {
            let risk = (entry - sl).abs();
            let reward = (tp - entry).abs();
            if risk > 0.0 {
                let rr = reward / risk;
                chart
                    .draw_series(std::iter::once(Text::new(
                        format!("R:R = 1:{:.1}", rr),
                        (2, pmin + (pmax - pmin) * 0.03),
                        ("sans-serif", 13).into_font().color(&gold),
                    )))
                    .map_err(fmt_err)?;
            }
        }

        root.present().map_err(fmt_err)?;
    }

    // Read PNG file
    let mut file = std::fs::File::open(&tmp_path).map_err(|e| format!("open png: {}", e))?;
    let mut png_buf = Vec::new();
    file.read_to_end(&mut png_buf)
        .map_err(|e| format!("read png: {}", e))?;
    let _ = std::fs::remove_file(&tmp_path);

    debug!(
        "chart: {}x{} PNG {} bytes for {}",
        w,
        h,
        png_buf.len(),
        symbol
    );
    Ok(png_buf)
}

fn fmt_err<E: std::fmt::Display>(e: E) -> String {
    format!("{}", e)
}
