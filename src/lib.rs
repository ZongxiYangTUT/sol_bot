//! Solana DCA Bot - 自动定投 USDC 买 SOL/SPL Token
//!
//! 核心组件：钱包管理、RPC 通信、Jupiter Swap、收益计算

pub mod config;
pub mod plan;
pub mod pnl;
pub mod rpc;
pub mod swap;
pub mod wallet;
pub mod web;
