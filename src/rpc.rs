//! Solana RPC 通信：余额查询、发送交易

use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::CommitmentConfig;
use solana_sdk::signature::Signature;
use solana_sdk::transaction::VersionedTransaction;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct SolanaRpc {
    pub client: Arc<RpcClient>,
}

impl SolanaRpc {
    pub fn new(rpc_url: &str) -> Self {
        let client = RpcClient::new_with_timeout_and_commitment(
            rpc_url.to_string(),
            Duration::from_secs(30),
            CommitmentConfig::confirmed(),
        );
        SolanaRpc {
            client: Arc::new(client),
        }
    }

    /// 获取 SOL 余额（lamports）
    pub fn get_sol_balance(&self, pubkey: &solana_sdk::pubkey::Pubkey) -> anyhow::Result<u64> {
        self.client
            .get_balance(pubkey)
            .map_err(anyhow::Error::msg)
    }

    /// 获取 SPL Token 账户余额（raw amount，需按 decimals 换算）
    pub fn get_token_balance(
        &self,
        wallet: &solana_sdk::pubkey::Pubkey,
        mint: &solana_sdk::pubkey::Pubkey,
    ) -> anyhow::Result<u64> {
        use solana_client::rpc_response::UiAccountData;
        let accounts = self.client.get_token_accounts_by_owner(
            wallet,
            solana_client::rpc_request::TokenAccountsFilter::Mint(*mint),
        )?;
        let mut total = 0u64;
        for keyed in accounts {
            if let UiAccountData::Json(parsed) = keyed.account.data {
                if let Some(info) = parsed.parsed.get("info") {
                    if let Some(amount) = info.get("token_amount").and_then(|a| a.get("amount").and_then(|v| v.as_str())) {
                        total += amount.parse::<u64>()?;
                    }
                }
            }
        }
        Ok(total)
    }

    /// 发送已签名的 VersionedTransaction
    pub fn send_transaction(&self, tx: &VersionedTransaction) -> anyhow::Result<Signature> {
        let sig = self.client.send_transaction(tx)?;
        Ok(sig)
    }

    /// 确认交易（轮询）
    pub fn confirm_transaction(&self, sig: &Signature) -> anyhow::Result<bool> {
        self.client
            .confirm_transaction(sig)
            .map_err(anyhow::Error::msg)
    }
}
