use anyhow::Result;
use clap::{Parser, Subcommand};
use northflow_crypto_trading_bot::{
    config::ResearchConfig,
    report::RunReport,
    research::run_backtest,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "northflow",
    about = "Northflow — research-first deterministic crypto trading bot",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a backtest from a TOML config file
    Backtest {
        /// Path to research.toml config
        #[arg(short, long, default_value = "config/research.toml")]
        config: PathBuf,
    },
    /// Show default config template
    Init,
    /// Paper trading mode (DISABLED — not yet implemented)
    #[command(hide = true)]
    Paper,
    /// Live trading mode (DISABLED — not yet implemented)
    #[command(hide = true)]
    Live,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Backtest { config } => {
            log::info!("Loading config from {}", config.display());
            let cfg = ResearchConfig::from_toml(&config)?;
            std::fs::create_dir_all(&cfg.report.output_dir)?;

            let result = run_backtest(&cfg)?;

            if cfg.report.json {
                let json_path = format!("{}/report.json", cfg.report.output_dir);
                result.report.write_json(&json_path)?;
                log::info!("Report written to {}", json_path);
            }
            if cfg.report.csv {
                let csv_path = format!("{}/trades.csv", cfg.report.output_dir);
                RunReport::write_trades_csv(&result.trades, &csv_path)?;
                log::info!("Trades written to {}", csv_path);
            }

            println!("\n=== Northflow Backtest Results ===");
            println!("Symbol:        {}", cfg.symbol);
            println!("Strategy:      {}", cfg.strategy.name);
            println!("Total Trades:  {}", result.report.total_trades);
            println!("Win Rate:      {:.1}%", result.report.win_rate * 100.0);
            println!("Total PnL:     {:.4}", result.report.total_pnl);
            println!("Max Drawdown:  {:.2}%", result.report.max_drawdown * 100.0);
            println!("Sharpe Ratio:  {:.4}", result.report.sharpe_ratio);
            println!("Return:        {:.2}%", result.report.return_pct);
            println!("Final Capital: {:.2}", result.report.final_capital);
            println!("==================================\n");
        }
        Commands::Init => {
            let default = ResearchConfig::default();
            let toml_str = toml::to_string_pretty(&default)?;
            println!("# Paste this into config/research.toml\n\n{}", toml_str);
        }
        Commands::Paper => {
            eprintln!("Paper trading is disabled until the research engine is validated.");
            std::process::exit(1);
        }
        Commands::Live => {
            eprintln!("Live trading is disabled until the research engine is validated.");
            std::process::exit(1);
        }
    }

    Ok(())
}
