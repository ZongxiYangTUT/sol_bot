//! 配置：RPC、钱包路径、定投金额、数据目录等

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::path::PathBuf;
use std::str::FromStr;

// ---------- 主网 (Mainnet) ----------
/// 主网 USDC
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
/// Wrapped SOL（主网/测试网地址相同）
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
/// 主网 RPC（默认）
pub const MAINNET_RPC_URL: &str = "https://api.mainnet-beta.solana.com";

// ---------- 测试网 (Devnet)，用于验证流程、无需真金白银 ----------
/// Devnet USDC（Circle 测试用）
pub const DEVNET_USDC_MINT: &str = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
/// Devnet RPC（官方；若被墙可改用 DEVNET_RPC_ANKR 或 --rpc-url 指定）
pub const DEVNET_RPC_URL: &str = "https://api.devnet.solana.com";
/// 备选 Devnet RPC（Ankr 公共节点，国内可能更易访问）
pub const DEVNET_RPC_ANKR: &str = "https://rpc.ankr.com/solana_devnet";

/// 默认使用主网（可被环境变量/配置文件覆盖）
pub const DEFAULT_RPC_URL: &str = MAINNET_RPC_URL;

// ---------- Jupiter API（jup_ag 通过环境变量 QUOTE_API_URL 读取）---------
/// Jupiter Metis Swap API（与官方文档一致：https://dev.jup.ag/docs/swap-api/get-quote）
/// Quote: GET {base}/quote?inputMint=...&outputMint=...&amount=...&slippageBps=...
/// Swap:  POST {base}/swap
pub const JUPITER_QUOTE_API_V1: &str = "https://api.jup.ag/swap/v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// Solana RPC URL
    pub rpc_url: String,
    /// 钱包 keypair 路径（JSON 数组）
    pub keypair_path: PathBuf,
    /// 每次定投消耗的 USDC 数量（人类可读，如 10 表示 10 USDC）
    pub usdc_amount_per_buy: f64,
    /// 输入 Token mint（默认 USDC）
    pub input_mint: String,
    /// 输出 Token mint（默认 SOL）
    pub output_mint: String,
    /// 定投间隔秒数（例如 86400 = 每天）
    pub interval_secs: u64,
    /// 滑点 bps（如 50 = 0.5%）
    pub slippage_bps: u64,
    /// Jupiter API Key（可选，部分场景需要）
    pub jupiter_api_key: Option<String>,
    /// Jupiter Quote/Swap API 的 base URL（jup_ag 会读环境变量 QUOTE_API_URL；此处若设则启动时写入 env）
    /// 默认 v6：https://quote-api.jup.ag/v6，避免 v1 返回格式变化导致 missing field inputMint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jupiter_quote_api_url: Option<String>,
    /// 买入记录与统计数据存放目录
    pub data_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self::mainnet_default()
    }
}

impl Config {
    /// 主网默认配置（当前默认）
    pub fn mainnet_default() -> Self {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("sol_bot");
        Config {
            rpc_url: MAINNET_RPC_URL.to_string(),
            keypair_path: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config/solana/id.json"),
            usdc_amount_per_buy: 10.0,
            input_mint: USDC_MINT.to_string(),
            output_mint: WSOL_MINT.to_string(),
            interval_secs: 86400,
            slippage_bps: 50,
            jupiter_api_key: None,
            jupiter_quote_api_url: Some(JUPITER_QUOTE_API_V1.to_string()),
            data_dir,
        }
    }

    /// 测试网 (Devnet) 配置，用于验证流程（无真金白银）
    /// 注意：Jupiter 聚合器主要支持主网，devnet 上 swap 可能不可用或需单独 API
    pub fn devnet_default() -> Self {
        let mut cfg = Self::mainnet_default();
        cfg.rpc_url = DEVNET_RPC_URL.to_string();
        cfg.input_mint = DEVNET_USDC_MINT.to_string();
        cfg.data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("sol_bot_devnet");
        cfg
    }

    /// 在调用 jup_ag 前调用，将 Jupiter API base URL 写入环境变量 QUOTE_API_URL（与官方文档一致）
    pub fn apply_jupiter_env(&self) {
        let url = self
            .jupiter_quote_api_url
            .as_deref()
            .unwrap_or(JUPITER_QUOTE_API_V1);
        std::env::set_var("QUOTE_API_URL", url);
    }
}

impl Config {
    pub fn input_mint_pubkey(&self) -> anyhow::Result<Pubkey> {
        Pubkey::from_str(&self.input_mint).map_err(anyhow::Error::msg)
    }

    pub fn output_mint_pubkey(&self) -> anyhow::Result<Pubkey> {
        Pubkey::from_str(&self.output_mint).map_err(anyhow::Error::msg)
    }

    /// USDC 为 6 位小数；其他 SPL 需根据 mint 配置
    pub fn usdc_amount_raw(&self) -> u64 {
        const USDC_DECIMALS: u32 = 6;
        (self.usdc_amount_per_buy * 10_f64.powi(USDC_DECIMALS as i32)) as u64
    }
}
