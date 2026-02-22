//! 钱包管理：从本地 JSON 加载 keypair

use solana_sdk::signature::Keypair;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("无效的 keypair JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("无效的公钥/格式: {0}")]
    InvalidPubkey(String),
}

/// 从 Solana CLI 格式的 JSON 文件加载 keypair（字节数组，通常 64 字节）
pub fn load_keypair_from_path<P: AsRef<Path>>(path: P) -> Result<Keypair, WalletError> {
    let path = path.as_ref();
    let data = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            WalletError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Keypair 文件不存在: {}。\n  解决：安装 Solana CLI 后执行 solana-keygen new，或用 -k/--keypair / 环境变量 SOLANA_KEYPAIR_PATH 指定已有 keypair 路径",
                    path.display()
                ),
            ))
        } else {
            WalletError::Io(std::io::Error::new(
                e.kind(),
                format!("读取 keypair 失败 {}: {}", path.display(), e),
            ))
        }
    })?;
    let bytes: Vec<u8> = serde_json::from_str(&data)?;
    if bytes.len() < 32 {
        return Err(WalletError::InvalidPubkey("keypair 至少需要 32 字节".to_string()));
    }
    let mut secret = [0u8; 32];
    secret.copy_from_slice(&bytes[..32]);
    Ok(Keypair::new_from_array(secret))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_invalid_path() {
        let r = load_keypair_from_path("/nonexistent/id.json");
        assert!(r.is_err());
    }
}
