//! 收益计算模块：记录每笔买入、成本、当前市值与盈亏

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 单笔买入记录
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuyRecord {
    /// 时间
    pub time: DateTime<Utc>,
    /// 花费的 input 数量（raw，如 USDC 6 位小数）
    pub input_amount_raw: u64,
    /// 得到的 output 数量（raw，如 SOL 9 位小数）
    pub output_amount_raw: u64,
    /// 成交价（input per unit output，用于显示）
    pub price_per_unit: f64,
    /// 交易签名
    pub signature: String,
    /// 所属定投计划 ID（可选，用于按计划统计）
    #[serde(default)]
    pub plan_id: Option<String>,
}

/// 持久化数据结构
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PnlStore {
    pub buys: Vec<BuyRecord>,
}

impl PnlStore {
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let data = std::fs::read_to_string(path.as_ref())?;
        let store: PnlStore = serde_json::from_str(&data)?;
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

    pub fn add_buy(&mut self, record: BuyRecord) {
        self.buys.push(record);
    }

    /// 总投入（input 人类可读，如 USDC）；可选按 plan_id 过滤
    pub fn total_input_human(&self, input_decimals: u8, plan_id: Option<&str>) -> f64 {
        let div = 10_f64.powi(input_decimals as i32);
        self.buys
            .iter()
            .filter(|b| plan_id.map_or(true, |id| b.plan_id.as_deref() == Some(id)))
            .map(|b| b.input_amount_raw as f64 / div)
            .sum()
    }

    /// 总获得 output（raw）；可选按 plan_id 过滤
    pub fn total_output_raw(&self, plan_id: Option<&str>) -> u64 {
        self.buys
            .iter()
            .filter(|b| plan_id.map_or(true, |id| b.plan_id.as_deref() == Some(id)))
            .map(|b| b.output_amount_raw)
            .sum()
    }

    /// 总获得 output（人类可读）；可选按 plan_id 过滤
    pub fn total_output_human(&self, output_decimals: u8, plan_id: Option<&str>) -> f64 {
        let div = 10_f64.powi(output_decimals as i32);
        self.total_output_raw(plan_id) as f64 / div
    }

    /// 成本均价（input per unit output）；可选按 plan_id 过滤
    pub fn avg_cost_per_unit(&self, plan_id: Option<&str>) -> f64 {
        let out = self.total_output_raw(plan_id);
        if out == 0 {
            return 0.0;
        }
        let in_raw: u64 = self
            .buys
            .iter()
            .filter(|b| plan_id.map_or(true, |id| b.plan_id.as_deref() == Some(id)))
            .map(|b| b.input_amount_raw)
            .sum();
        (in_raw as f64) / (out as f64)
    }

    /// 某计划的买入笔数
    pub fn buy_count(&self, plan_id: &str) -> usize {
        self.buys
            .iter()
            .filter(|b| b.plan_id.as_deref() == Some(plan_id))
            .count()
    }
}

/// 收益统计（用于 CLI 输出）
#[derive(Debug)]
pub struct PnlSummary {
    pub total_buys: usize,
    pub total_input_human: f64,
    pub total_output_human: f64,
    pub avg_cost_per_unit: f64,
    pub current_price_per_unit: f64,
    pub current_value_human: f64,
    pub pnl_absolute: f64,
    pub pnl_percent: f64,
}

/// 根据当前价格计算收益；plan_id 为 None 时统计全部，为 Some 时仅统计该计划
pub fn compute_pnl(
    store: &PnlStore,
    input_decimals: u8,
    output_decimals: u8,
    current_price_per_unit: f64,
    plan_id: Option<&str>,
) -> PnlSummary {
    let total_buys = store
        .buys
        .iter()
        .filter(|b| plan_id.map_or(true, |id| b.plan_id.as_deref() == Some(id)))
        .count();
    let total_input_human = store.total_input_human(input_decimals, plan_id);
    let total_output_human = store.total_output_human(output_decimals, plan_id);
    let avg_cost_per_unit = if total_output_human > 0.0 {
        total_input_human / total_output_human
    } else {
        0.0
    };
    let current_value_human = total_output_human * current_price_per_unit;
    let pnl_absolute = current_value_human - total_input_human;
    let pnl_percent = if total_input_human > 0.0 {
        (pnl_absolute / total_input_human) * 100.0
    } else {
        0.0
    };

    PnlSummary {
        total_buys,
        total_input_human,
        total_output_human,
        avg_cost_per_unit,
        current_price_per_unit,
        current_value_human,
        pnl_absolute,
        pnl_percent,
    }
}
