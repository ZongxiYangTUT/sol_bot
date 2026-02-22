//! Solana DCA Bot CLI
//!
//! 子命令：run（定时执行）、buy（单次买入）、stats（收益统计）

use clap::{Parser, Subcommand};
use sol_bot::{config::Config, pnl, rpc, swap, wallet, web};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "sol_bot")]
#[command(about = "Solana DCA Bot - 用 USDC 自动定投买 SOL/SPL Token", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// 配置文件路径（可选）
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// RPC URL（覆盖配置，也可用环境变量 SOLANA_RPC_URL）
    #[arg(long, global = true)]
    rpc_url: Option<String>,

    /// 钱包 keypair 路径（覆盖配置，也可用环境变量 SOLANA_KEYPAIR_PATH）
    #[arg(short, long, global = true)]
    keypair: Option<PathBuf>,

    /// 使用测试网 Devnet（RPC + Devnet USDC，数据目录单独为 sol_bot_devnet）
    #[arg(long, global = true)]
    devnet: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// 按间隔定时执行买入（默认每天一次）
    Run {
        /// 仅运行一次然后退出（不循环）
        #[arg(long)]
        once: bool,
    },

    /// 立即执行一次买入
    Buy,

    /// 输出当前收益统计
    Stats,

    /// 启动 Web 界面（浏览器打开）
    Serve {
        /// 监听端口
        #[arg(short, long, default_value = "3030")]
        port: u16,
    },
}

fn load_config(cli: &Cli) -> anyhow::Result<Config> {
    let base = if cli.devnet {
        Config::devnet_default()
    } else {
        Config::mainnet_default()
    };
    let mut cfg: Config = if let Some(p) = &cli.config {
        let data = std::fs::read_to_string(p)?;
        serde_json::from_str(&data).unwrap_or(base)
    } else {
        base
    };
    if let Some(url) = cli.rpc_url.clone().or_else(|| std::env::var("SOLANA_RPC_URL").ok()) {
        cfg.rpc_url = url;
    }
    if let Some(kp) = cli.keypair.clone().or_else(|| std::env::var("SOLANA_KEYPAIR_PATH").ok().map(PathBuf::from)) {
        cfg.keypair_path = kp;
    }
    if cfg.jupiter_api_key.as_deref().unwrap_or("").is_empty() {
        if let Ok(k) = std::env::var("JUPITER_API_KEY") {
            if !k.is_empty() {
                cfg.jupiter_api_key = Some(k);
            }
        }
    }
    Ok(cfg)
}

fn pnl_path(cfg: &Config) -> PathBuf {
    cfg.data_dir.join("pnl.json")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = load_config(&cli)?;

    cfg.apply_jupiter_env();
    std::fs::create_dir_all(&cfg.data_dir)?;
    if cli.devnet {
        eprintln!("[Devnet] 使用测试网: RPC={}, 数据目录={}", cfg.rpc_url, cfg.data_dir.display());
    }
    let api_key = cfg
        .jupiter_api_key
        .as_deref()
        .unwrap_or("")
        .to_string();

    match cli.command {
        Commands::Run { once } => {
            let keypair = wallet::load_keypair_from_path(&cfg.keypair_path)?;
            let rpc = rpc::SolanaRpc::new(&cfg.rpc_url);
            let input_mint = cfg.input_mint_pubkey()?;
            let output_mint = cfg.output_mint_pubkey()?;
            let amount_raw = cfg.usdc_amount_raw();
            let slippage_bps = cfg.slippage_bps;

            loop {
                if let Err(e) = do_buy(
                    &cfg,
                    &keypair,
                    &rpc,
                    &api_key,
                    input_mint,
                    output_mint,
                    amount_raw,
                    slippage_bps,
                )
                .await
                {
                    eprintln!("买入失败: {}", e);
                }
                if once {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(cfg.interval_secs)).await;
            }
        }

        Commands::Buy => {
            let keypair = wallet::load_keypair_from_path(&cfg.keypair_path)?;
            let rpc = rpc::SolanaRpc::new(&cfg.rpc_url);
            let input_mint = cfg.input_mint_pubkey()?;
            let output_mint = cfg.output_mint_pubkey()?;
            let amount_raw = cfg.usdc_amount_raw();
            let slippage_bps = cfg.slippage_bps;

            do_buy(
                &cfg,
                &keypair,
                &rpc,
                &api_key,
                input_mint,
                output_mint,
                amount_raw,
                slippage_bps,
            )
            .await?;
        }

        Commands::Stats => {
            let path = pnl_path(&cfg);

            let store = if path.exists() {
                pnl::PnlStore::load_from_path(&path)?
            } else {
                pnl::PnlStore::default()
            };

            // 当前价：用 Jupiter 或 RPC 查；这里简化为用最近一笔买入价近似，或从 RPC 取余额 * 市价
            // 更准确做法是调 Jupiter price API；此处用最后一笔成交价作为「当前价」近似
            let current_price = store
                .buys
                .last()
                .map(|b| b.price_per_unit)
                .unwrap_or(0.0);

            // 若有输出 token 余额，可用 get_token_balance 配合外部价格；此处用历史均价
            let (current_price_used, current_value_msg) = if current_price > 0.0 {
                (current_price, format!("(当前价近似 {:.4} USDC/单位)", current_price))
            } else {
                (0.0, "(无历史买入，无法估算当前价)".to_string())
            };

            const INPUT_DECIMALS: u8 = 6;  // USDC
            const OUTPUT_DECIMALS: u8 = 9;  // SOL

            let summary = pnl::compute_pnl(
                &store,
                INPUT_DECIMALS,
                OUTPUT_DECIMALS,
                current_price_used,
            );

            println!("========== DCA 收益统计 ==========");
            println!("买入次数: {}", summary.total_buys);
            println!("累计投入 (USDC): {:.2}", summary.total_input_human);
            println!("累计获得 (output): {:.6}", summary.total_output_human);
            println!("成本均价: {:.4} USDC/单位", summary.avg_cost_per_unit);
            println!("当前价: {} {}", current_price_used, current_value_msg);
            println!("当前市值 (USDC): {:.2}", summary.current_value_human);
            println!("浮动盈亏: {:.2} USDC ({:.2}%)", summary.pnl_absolute, summary.pnl_percent);
            println!("====================================");
        }

        Commands::Serve { port } => {
            let keypair = wallet::load_keypair_from_path(&cfg.keypair_path)?;
            let rpc = rpc::SolanaRpc::new(&cfg.rpc_url);
            let state = Arc::new(web::AppState {
                config: cfg.clone(),
                keypair,
                rpc,
                api_key: api_key.clone(),
                last_message: tokio::sync::RwLock::new(String::new()),
            });
            let app = web::router(state);
            let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
            println!("Web 界面: http://127.0.0.1:{}/", port);
            let listener = tokio::net::TcpListener::bind(addr).await?;
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}

async fn do_buy(
    cfg: &Config,
    keypair: &solana_sdk::signature::Keypair,
    rpc: &rpc::SolanaRpc,
    api_key: &str,
    input_mint: solana_sdk::pubkey::Pubkey,
    output_mint: solana_sdk::pubkey::Pubkey,
    amount_raw: u64,
    slippage_bps: u64,
) -> anyhow::Result<()> {
    let (quote, sig) = swap::quote_and_swap(
        input_mint,
        output_mint,
        amount_raw,
        slippage_bps,
        keypair,
        rpc,
        api_key,
    )
    .await
    .map_err(anyhow::Error::msg)?;

    let price_per_unit = if quote.out_amount > 0 {
        (quote.in_amount as f64) / (quote.out_amount as f64)
    } else {
        0.0
    };

    let record = pnl::BuyRecord {
        time: chrono::Utc::now(),
        input_amount_raw: quote.in_amount,
        output_amount_raw: quote.out_amount,
        price_per_unit,
        signature: sig.to_string(),
    };

    let path = pnl_path(cfg);
    let mut store = if path.exists() {
        pnl::PnlStore::load_from_path(&path)?
    } else {
        pnl::PnlStore::default()
    };
    store.add_buy(record);
    store.save_to_path(&path)?;

    println!("买入成功 tx: {}", sig);
    Ok(())
}
