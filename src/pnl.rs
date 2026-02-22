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

    /// 总投入（input 人类可读，如 USDC）
    pub fn total_input_human(&self, input_decimals: u8) -> f64 {
        let div = 10_f64.powi(input_decimals as i32);
        self.buys.iter().map(|b| b.input_amount_raw as f64 / div).sum()
    }

    /// 总获得 output（raw）
    pub fn total_output_raw(&self) -> u64 {
        self.buys.iter().map(|b| b.output_amount_raw).sum()
    }

    /// 总获得 output（人类可读）
    pub fn total_output_human(&self, output_decimals: u8) -> f64 {
        let div = 10_f64.powi(output_decimals as i32);
        self.total_output_raw() as f64 / div
    }

    /// 成本均价（input per unit output）
    pub fn avg_cost_per_unit(&self) -> f64 {
        let out: u64 = self.total_output_raw();
        if out == 0 {
            return 0.0;
        }
        let in_raw: u64 = self.buys.iter().map(|b| b.input_amount_raw).sum();
        // 需要按 decimals 换算成同一单位再除；这里返回「每 1 个 output 需要多少 input raw 比例」
        // 实际用人类可读时：total_input_human / total_output_human
        (in_raw as f64) / (out as f64)
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

/// 根据当前价格计算收益
pub fn compute_pnl(
    store: &PnlStore,
    input_decimals: u8,
    output_decimals: u8,
    current_price_per_unit: f64,
) -> PnlSummary {
    let total_buys = store.buys.len();
    let total_input_human = store.total_input_human(input_decimals);
    let total_output_human = store.total_output_human(output_decimals);
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
