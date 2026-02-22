//! Web 服务：提供 API 与静态页面，浏览器可打开

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use solana_sdk::signer::Signer;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::pnl::{self, PnlSummary};
use crate::rpc;
use crate::swap;

/// 共享状态
pub struct AppState {
    pub config: Config,
    pub keypair: solana_sdk::signature::Keypair,
    pub rpc: rpc::SolanaRpc,
    pub api_key: String,
    /// 最后一次操作结果（用于 UI 提示）
    pub last_message: RwLock<String>,
}

fn pnl_path(cfg: &Config) -> PathBuf {
    cfg.data_dir.join("pnl.json")
}

const INPUT_DECIMALS: u8 = 6;
const OUTPUT_DECIMALS: u8 = 9;

// ---------- API 响应类型 ----------

#[derive(serde::Serialize)]
pub struct BalanceResponse {
    pub wallet: String,
    pub sol_balance: f64,
    pub usdc_balance: f64,
    pub sol_lamports: u64,
    pub usdc_raw: u64,
}

#[derive(serde::Serialize)]
pub struct StatsResponse {
    pub total_buys: usize,
    pub total_input_human: f64,
    pub total_output_human: f64,
    pub avg_cost_per_unit: f64,
    pub current_price_per_unit: f64,
    pub current_value_human: f64,
    pub pnl_absolute: f64,
    pub pnl_percent: f64,
}

#[derive(serde::Serialize)]
pub struct BuyRecordDto {
    pub time: String,
    pub input_amount: f64,
    pub output_amount: f64,
    pub price_per_unit: f64,
    pub signature: String,
}

#[derive(serde::Serialize)]
pub struct BuysResponse {
    pub buys: Vec<BuyRecordDto>,
}

#[derive(serde::Serialize)]
pub struct ConfigResponse {
    pub rpc_url: String,
    pub usdc_amount_per_buy: f64,
    pub interval_secs: u64,
    pub slippage_bps: u64,
}

#[derive(serde::Serialize)]
pub struct BuyResultResponse {
    pub success: bool,
    pub signature: Option<String>,
    pub error: Option<String>,
}

// ---------- 路由 ----------

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn api_balance(State(state): State<Arc<AppState>>) -> Result<Json<BalanceResponse>, ApiError> {
    let pubkey = state.keypair.pubkey();
    let sol_lamports = state
        .rpc
        .get_sol_balance(&pubkey)
        .map_err(|e| ApiError(e.to_string()))?;
    let input_mint = state.config.input_mint_pubkey().map_err(|e| ApiError(e.to_string()))?;
    let usdc_raw = state
        .rpc
        .get_token_balance(&pubkey, &input_mint)
        .unwrap_or(0);

    Ok(Json(BalanceResponse {
        wallet: pubkey.to_string(),
        sol_balance: sol_lamports as f64 / 1_000_000_000.0,
        usdc_balance: usdc_raw as f64 / 1_000_000.0,
        sol_lamports,
        usdc_raw,
    }))
}

async fn api_stats(State(state): State<Arc<AppState>>) -> Result<Json<StatsResponse>, ApiError> {
    let path = pnl_path(&state.config);
    let store = if path.exists() {
        pnl::PnlStore::load_from_path(&path).unwrap_or_default()
    } else {
        pnl::PnlStore::default()
    };

    let current_price = store
        .buys
        .last()
        .map(|b| b.price_per_unit)
        .unwrap_or(0.0);

    let summary: PnlSummary = pnl::compute_pnl(&store, INPUT_DECIMALS, OUTPUT_DECIMALS, current_price);

    Ok(Json(StatsResponse {
        total_buys: summary.total_buys,
        total_input_human: summary.total_input_human,
        total_output_human: summary.total_output_human,
        avg_cost_per_unit: summary.avg_cost_per_unit,
        current_price_per_unit: summary.current_price_per_unit,
        current_value_human: summary.current_value_human,
        pnl_absolute: summary.pnl_absolute,
        pnl_percent: summary.pnl_percent,
    }))
}

async fn api_buys(State(state): State<Arc<AppState>>) -> Result<Json<BuysResponse>, ApiError> {
    let path = pnl_path(&state.config);
    let store = if path.exists() {
        pnl::PnlStore::load_from_path(&path).unwrap_or_default()
    } else {
        pnl::PnlStore::default()
    };

    let buys: Vec<BuyRecordDto> = store
        .buys
        .iter()
        .rev()
        .take(20)
        .map(|b| BuyRecordDto {
            time: b.time.format("%Y-%m-%d %H:%M UTC").to_string(),
            input_amount: b.input_amount_raw as f64 / 10_f64.powi(INPUT_DECIMALS as i32),
            output_amount: b.output_amount_raw as f64 / 10_f64.powi(OUTPUT_DECIMALS as i32),
            price_per_unit: b.price_per_unit,
            signature: b.signature.clone(),
        })
        .collect();

    Ok(Json(BuysResponse { buys }))
}

async fn api_buy(State(state): State<Arc<AppState>>) -> Result<Json<BuyResultResponse>, ApiError> {
    let input_mint = state.config.input_mint_pubkey().map_err(|e| ApiError(e.to_string()))?;
    let output_mint = state.config.output_mint_pubkey().map_err(|e| ApiError(e.to_string()))?;
    let amount_raw = state.config.usdc_amount_raw();
    let slippage_bps = state.config.slippage_bps;

    match swap::quote_and_swap(
        input_mint,
        output_mint,
        amount_raw,
        slippage_bps,
        &state.keypair,
        &state.rpc,
        &state.api_key,
    )
    .await
    {
        Ok((quote, sig)) => {
            let price_per_unit = if quote.out_amount > 0 {
                quote.in_amount as f64 / quote.out_amount as f64
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
            let path = pnl_path(&state.config);
            let mut store = if path.exists() {
                pnl::PnlStore::load_from_path(&path).unwrap_or_default()
            } else {
                pnl::PnlStore::default()
            };
            store.add_buy(record);
            let _ = store.save_to_path(&path);

            {
                let mut msg = state.last_message.write().await;
                *msg = format!("买入成功 tx: {}", sig);
            }

            Ok(Json(BuyResultResponse {
                success: true,
                signature: Some(sig.to_string()),
                error: None,
            }))
        }
        Err(e) => {
            let err_str = e.to_string();
            let hint = if err_str.contains("error sending request") {
                "（若在国内网络，可设置 HTTPS_PROXY 代理后重试，见 README）"
            } else {
                ""
            };
            {
                let mut msg = state.last_message.write().await;
                *msg = format!("买入失败: {}{}", err_str, hint);
            }
            Ok(Json(BuyResultResponse {
                success: false,
                signature: None,
                error: Some(format!("{}{}", err_str, hint)),
            }))
        }
    }
}

async fn api_config(State(state): State<Arc<AppState>>) -> Json<ConfigResponse> {
    Json(ConfigResponse {
        rpc_url: state.config.rpc_url.clone(),
        usdc_amount_per_buy: state.config.usdc_amount_per_buy,
        interval_secs: state.config.interval_secs,
        slippage_bps: state.config.slippage_bps,
    })
}

async fn api_last_message(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let msg = state.last_message.read().await.clone();
    Json(serde_json::json!({ "message": msg }))
}

// ---------- 错误处理 ----------

struct ApiError(String);

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": self.0 })),
        )
            .into_response()
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        ApiError(e.to_string())
    }
}

// ---------- 启动 ----------

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/balance", get(api_balance))
        .route("/api/stats", get(api_stats))
        .route("/api/buys", get(api_buys))
        .route("/api/buy", post(api_buy))
        .route("/api/config", get(api_config))
        .route("/api/last_message", get(api_last_message))
        .with_state(state)
}
