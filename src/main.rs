use northflow_crypto_trading_bot::{config::ResearchConfig, research::run_research};
use std::{env, process};

fn main() {
    if let Err(err) = real_main() {
        eprintln!("error: {err}");
        process::exit(1);
    }
}

fn real_main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    let command = args.get(1).map(String::as_str).unwrap_or("help");
    match command {
        "research" => {
            let config_path = read_config_arg(&args)
                .unwrap_or_else(|| "config/research.toml".to_string());
            let cfg = ResearchConfig::load(&config_path)?;
            run_research(&cfg)
        }
        "paper" => Err("paper mode is disabled until research engine is validated".to_string()),
        "live"  => Err("live mode is disabled until paper/live parity is proven".to_string()),
        _ => {
            print_help();
            Ok(())
        }
    }
}

fn read_config_arg(args: &[String]) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == "--config" || pair[0] == "-c")
        .map(|pair| pair[1].clone())
}

fn print_help() {
    println!("Northflow Crypto Trading Bot");
    println!();
    println!("Usage:");
    println!("  northflow research [--config config/research.toml]");
    println!("  northflow paper   # disabled — not yet validated");
    println!("  northflow live    # disabled — not yet validated");
    println!();
    println!("Data: place CSV files in data/historical/<SYMBOL>.csv");
    println!("      columns: timestamp,open,high,low,close,volume");
}
