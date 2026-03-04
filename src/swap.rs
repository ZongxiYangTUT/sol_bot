//! 通过 Jupiter Aggregator 执行 Swap（USDC -> SOL 等）

use jup_ag::{Quote, SwapRequest};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::VersionedTransaction;

use crate::config::JUPITER_QUOTE_API_V1;
use crate::rpc::SolanaRpc;

#[derive(Debug, thiserror::Error)]
pub enum SwapError {
    #[error("Jupiter API 失败: {0}")]
    Jupiter(#[from] jup_ag::Error),
    #[error("RPC 失败: {0}")]
    Rpc(#[from] anyhow::Error),
    #[error("签名失败")]
    Sign,
}

/// 自己发 Quote 请求并解析，先识别 API 返回的错误 JSON（error/message），再解析 Quote，避免 jup_ag 把错误体当 Quote 导致 missing field inputMint
async fn get_quote_http(
    input_mint: Pubkey,
    output_mint: Pubkey,
    amount_raw: u64,
    slippage_bps: u64,
    api_key: &str,
) -> Result<Quote, SwapError> {
    let base = std::env::var("QUOTE_API_URL").unwrap_or_else(|_| JUPITER_QUOTE_API_V1.to_string());
    let url = format!(
        "{}/quote?inputMint={}&outputMint={}&amount={}&onlyDirectRoutes=false&swapMode=ExactIn&slippageBps={}",
        base.trim_end_matches('/'),
        input_mint,
        output_mint,
        amount_raw,
        slippage_bps,
    );
    // println!("url: {}", url);
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| SwapError::Rpc(anyhow::Error::msg(e.to_string())))?;

    let mut req = client.get(&url);
    if !api_key.is_empty() {
        req = req.header("x-api-key", api_key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| SwapError::Rpc(anyhow::Error::msg(e.to_string())))?;

    let status = resp.status();
    let value: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| SwapError::Rpc(anyhow::Error::msg(e.to_string())))?;

    if !status.is_success() {
        let msg = value
            .get("error")
            .or_else(|| value.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误");
        let hint = if status == 401 || msg.to_lowercase().contains("unauthorized") {
            " 请在 https://portal.jup.ag/api-keys 申请 API Key，并在配置 jupiter_api_key 或环境变量 JUPITER_API_KEY 中设置。"
        } else {
            ""
        };
        return Err(SwapError::Jupiter(jup_ag::Error::JupiterApi(format!(
            "{}{}",
            msg, hint
        ))));
    }

    if value.get("inputMint").is_none() {
        let msg = value
            .get("error")
            .or_else(|| value.get("message"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                format!(
                    "API 返回格式异常（缺少 inputMint），可能为错误响应。原始片段: {}",
                    serde_json::to_string(&value)
                        .unwrap_or_else(|_| "?".to_string())
                        .chars()
                        .take(200)
                        .collect::<String>()
                )
            });
        return Err(SwapError::Jupiter(jup_ag::Error::JupiterApi(msg)));
    }

    let quote: Quote = serde_json::from_value(value).map_err(|e| SwapError::Jupiter(e.into()))?;
    Ok(quote)
}

/// 获取报价
pub async fn get_quote(
    input_mint: Pubkey,
    output_mint: Pubkey,
    amount_raw: u64,
    slippage_bps: u64,
    api_key: &str,
) -> Result<Quote, SwapError> {
    get_quote_http(input_mint, output_mint, amount_raw, slippage_bps, api_key).await
}

/// 获取 swap 交易并签名、发送
pub async fn execute_swap(
    quote: Quote,
    keypair: &Keypair,
    rpc: &SolanaRpc,
    api_key: &str,
) -> Result<solana_sdk::signature::Signature, SwapError> {
    let swap_request = SwapRequest::new(keypair.pubkey(), quote);
    let swap_result = jup_ag::swap(swap_request, api_key.to_string()).await?;

    let recent_blockhash = rpc
        .client
        .get_latest_blockhash()
        .map_err(anyhow::Error::msg)?;
    let mut message = swap_result.swap_transaction.message;
    message.set_recent_blockhash(recent_blockhash);

    let tx = VersionedTransaction::try_new(message, &[keypair]).map_err(|_| SwapError::Sign)?;

    let sig = rpc.send_transaction(&tx)?;
    Ok(sig)
}

/// 一站式：报价 + 执行（不做余额预检查，由链上模拟/执行时校验）
pub async fn quote_and_swap(
    input_mint: Pubkey,
    output_mint: Pubkey,
    amount_raw: u64,
    slippage_bps: u64,
    keypair: &Keypair,
    rpc: &SolanaRpc,
    api_key: &str,
) -> Result<(Quote, solana_sdk::signature::Signature), SwapError> {
    let quote = get_quote(input_mint, output_mint, amount_raw, slippage_bps, api_key).await?;
    let sig = execute_swap(quote.clone(), keypair, rpc, api_key).await?;
    Ok((quote, sig))
}
