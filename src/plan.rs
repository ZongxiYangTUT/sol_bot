//! 定投计划：创建、存储、调度与执行

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::str::FromStr;

use crate::pnl;
use crate::swap;

/// 单一定投计划
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub name: String,
    /// 输入代币 mint 地址（如 USDC）
    pub input_mint: String,
    /// 输出代币 mint 地址（如 SOL）
    pub output_mint: String,
    /// 每期定投金额（人类可读，如 10.0 = 10 USDC）
    pub amount_per_period: f64,
    /// 定投间隔秒数（如 86400 = 每日）
    pub interval_secs: u64,
    /// 是否启用（可手动开启/关闭）
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    /// 下次执行时间
    pub next_run_at: DateTime<Utc>,
    /// 已触发次数
    pub trigger_count: u64,
    /// 上次执行时间（用于界面显示是否触发）
    #[serde(default)]
    pub last_run_at: Option<DateTime<Utc>>,
    /// 上次是否执行成功
    #[serde(default)]
    pub last_run_ok: Option<bool>,
    /// 上次执行结果信息（成功时为 tx 签名，失败时为错误信息）
    #[serde(default)]
    pub last_run_message: Option<String>,
}

impl Plan {
    /// 每期金额 raw（按 USDC 6 位小数）
    pub fn amount_raw(&self) -> u64 {
        const DECIMALS: u32 = 6;
        (self.amount_per_period * 10_f64.powi(DECIMALS as i32)) as u64
    }

    /// 执行后更新下次运行时间
    pub fn advance_next_run(&mut self) {
        self.next_run_at = Utc::now() + chrono::Duration::seconds(self.interval_secs as i64);
        self.trigger_count += 1;
    }

    /// 记录上次执行结果（成功或失败都推进下次时间，避免失败时每分钟重试）
    pub fn set_last_run(&mut self, ok: bool, message: Option<String>) {
        self.last_run_at = Some(Utc::now());
        self.last_run_ok = Some(ok);
        self.last_run_message = message;
        self.advance_next_run();
    }
}

/// 计划列表持久化
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlanStore {
    pub plans: Vec<Plan>,
}

impl PlanStore {
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(PlanStore::default());
        }
        let data = std::fs::read_to_string(path)?;
        let store: PlanStore = serde_json::from_str(&data)?;
        Ok(store)
    }

    pub fn save_to_path<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path.as_ref(), data)?;
        Ok(())
    }

    pub fn add(&mut self, plan: Plan) {
        self.plans.push(plan);
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Plan> {
        self.plans.iter_mut().find(|p| p.id == id)
    }

    pub fn remove(&mut self, id: &str) -> Option<Plan> {
        let i = self.plans.iter().position(|p| p.id == id)?;
        Some(self.plans.remove(i))
    }

    /// 所有已启用且到期的计划（next_run_at <= now）
    pub fn due_plans(&self, now: DateTime<Utc>) -> Vec<Plan> {
        self.plans
            .iter()
            .filter(|p| p.enabled && p.next_run_at <= now)
            .cloned()
            .collect()
    }
}

/// 生成新计划 ID
pub fn new_plan_id() -> String {
    format!("plan_{}", Utc::now().timestamp_millis())
}

/// 执行一次定投计划：报价、swap、写入买入记录（带 plan_id）
pub async fn execute_plan(
    plan: &Plan,
    keypair: &solana_sdk::signature::Keypair,
    rpc: &crate::rpc::SolanaRpc,
    api_key: &str,
    slippage_bps: u64,
    pnl_path: &Path,
) -> anyhow::Result<solana_sdk::signature::Signature> {
    let input_mint = solana_sdk::pubkey::Pubkey::from_str(&plan.input_mint).map_err(anyhow::Error::msg)?;
    let output_mint = solana_sdk::pubkey::Pubkey::from_str(&plan.output_mint).map_err(anyhow::Error::msg)?;
    let amount_raw = plan.amount_raw();

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
        quote.in_amount as f64 / quote.out_amount as f64
    } else {
        0.0
    };

    let record = pnl::BuyRecord {
        time: Utc::now(),
        input_amount_raw: quote.in_amount,
        output_amount_raw: quote.out_amount,
        price_per_unit,
        signature: sig.to_string(),
        plan_id: Some(plan.id.clone()),
    };

    let mut store = if pnl_path.exists() {
        pnl::PnlStore::load_from_path(pnl_path)?
    } else {
        pnl::PnlStore::default()
    };
    store.add_buy(record);
    store.save_to_path(pnl_path)?;

    Ok(sig)
}
