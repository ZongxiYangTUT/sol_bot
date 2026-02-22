# Solana DCA Bot

基于 Solana 的自动定投 (DCA) Bot，使用 Rust 实现。支持用 USDC 定期买入 SOL 或任意 SPL Token（通过 Jupiter Aggregator），并记录买入与收益统计。

## 功能

- **钱包管理**：从本地 JSON keypair 加载（兼容 Solana CLI `~/.config/solana/id.json`）
- **Solana RPC**：余额查询、发送并确认交易
- **Swap**：通过 Jupiter Aggregator 获取报价并执行 USDC → SOL（或任意 SPL）兑换
- **收益模块**：记录每笔买入价格，统计累计投入、成本均价、浮动盈亏
- **CLI**：`run` 定时执行、`buy` 单次买入、`stats` 输出收益情况
- **Web 界面**：浏览器打开，查看余额/收益、一键买入、最近记录

## 依赖

- Rust 1.70+
- Solana 钱包（主网需 USDC + SOL 作 gas；测试网可用 Devnet SOL + Devnet USDC 水龙头）

## 主网 vs 测试网 (Devnet)

| 项目 | 默认（主网） | 测试网 `--devnet` |
|------|----------------|--------------------|
| RPC | `api.mainnet-beta.solana.com` | `api.devnet.solana.com` |
| USDC Mint | 主网 USDC | Devnet USDC（Circle 测试） |
| 数据目录 | `sol_bot` | `sol_bot_devnet`（与主网隔离） |

**默认是主网**。想用测试网验证流程、不花真钱时，加全局参数 `--devnet` 即可：

```bash
cargo run -- --devnet serve          # Web 界面连 Devnet
cargo run -- --devnet buy           # 在 Devnet 执行一次买入
cargo run -- --devnet stats         # 查看 Devnet 上的收益统计
```

测试网准备：Devnet SOL 用 `solana airdrop 2`（先 `solana config set --url devnet`），Devnet USDC 可从 [Circle Faucet](https://faucet.circle.com/) 等获取。**注意**：Jupiter 聚合器主要面向主网，Devnet 上 swap 可能不可用或需单独 API，若报错可先在主网小额验证。

**若 `api.devnet.solana.com` 请求失败（超时/被墙）**，可换用其它 Devnet RPC 再试 airdrop，例如：

```bash
# 指定可访问的 Devnet RPC（示例：Helius 等提供的 devnet 需自行申请；或尝试下面公共节点）
solana config set --url https://api.devnet.solana.com
# 若仍失败，换 RPC 后重试 airdrop：
solana config set --url https://rpc.ankr.com/solana_devnet
solana airdrop 2
```

本机跑 Bot 时同样可覆盖 RPC，例如：

```bash
cargo run -- --devnet --rpc-url https://rpc.ankr.com/solana_devnet serve
```

**不领测试币也能做的验证**：不依赖 airdrop，只要程序能启动、能连上 RPC 即可：

1. 生成 keypair：`solana-keygen new`（若还没有）。
2. 启动 Web：`cargo run -- serve`（主网）或 `cargo run -- --devnet serve`（测试网，RPC 需可达）。
3. 浏览器打开 http://127.0.0.1:3030/：能看到钱包地址、SOL/USDC 余额（可为 0）、收益统计（可为空）、最近买入（为空）。
4. 点「立即买入」：会因余额不足或 Jupiter 限制报错，属正常；说明前端→后端→RPC 链路是通的。

若要完整跑通一笔 swap，可在主网用极少金额（如 1 USDC + 少量 SOL 作 gas）验证。

## 配置

默认行为（主网，无需配置文件）：

- RPC: `https://api.mainnet-beta.solana.com`（建议自建或用付费 RPC）
- Keypair: `~/.config/solana/id.json`
- 每次定投: 10 USDC
- 输入 mint: USDC，输出 mint: Wrapped SOL
- 定投间隔: 86400 秒（1 天）
- 滑点: 50 bps (0.5%)
- 数据目录: 系统 `data_local_dir()/sol_bot`（如 macOS `~/Library/Application Support/sol_bot`）

可通过 `--config`、`--rpc-url`、`--keypair` 覆盖，或环境变量 `SOLANA_RPC_URL`、`SOLANA_KEYPAIR_PATH`。

可选配置文件（JSON）示例：

```json
{
  "rpc_url": "https://api.mainnet-beta.solana.com",
  "keypair_path": "~/.config/solana/id.json",
  "usdc_amount_per_buy": 10.0,
  "input_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  "output_mint": "So11111111111111111111111111111111111111112",
  "interval_secs": 86400,
  "slippage_bps": 50,
  "jupiter_api_key": "你的Jupiter_API_Key",
  "data_dir": "/path/to/sol_bot_data"
}
```

**Jupiter API 需要 API Key**（`api.jup.ag` 会返回 401 Unauthorized 若未提供）。请到 [Jupiter Customer Portal](https://portal.jup.ag/api-keys) 申请（有 Free 档），然后任选其一：

- 配置文件：将上述 `jupiter_api_key` 改为你的 Key；
- 环境变量：`export JUPITER_API_KEY=你的Key`，无需改配置文件。  
Jupiter Quote/Swap 使用官方 [Metis Swap API](https://dev.jup.ag/docs/swap-api/get-quote) base：`https://api.jup.ag/swap/v1`，启动时写入环境变量 `QUOTE_API_URL`；若需覆盖，可在配置中设置 `jupiter_quote_api_url`。

**若出现 `reqwest: error sending request for url (https://api.jup.ag/...)`**：说明本机无法直连 Jupiter API（超时或被墙）。可设置**代理**后重试：

```bash
# 示例：使用本地代理（端口按你的代理软件填写，如 7890、1087）
export HTTPS_PROXY=http://127.0.0.1:7890
cargo run --release -- serve
# 或点「立即买入」前在同一个终端里 export 后再启动
```

## 使用

```bash
# 编译
cargo build --release

# 单次立即买入
cargo run --release -- buy

# 定时定投（每 interval_secs 执行一次）
cargo run --release -- run

# 只执行一次然后退出
cargo run --release -- run --once

# 查看收益统计（当前价用最近一笔买入价近似）
cargo run --release -- stats

# 指定 RPC 与钱包
SOLANA_RPC_URL=https://your-rpc.com SOLANA_KEYPAIR_PATH=./key.json cargo run --release -- buy
# 或
cargo run --release -- --rpc-url https://your-rpc.com --keypair ./key.json buy

# 使用配置文件
cargo run --release -- --config config.json run

# 启动 Web 界面（默认端口 3030，浏览器打开）
cargo run --release -- serve
cargo run --release -- serve --port 8080
```

启动 `serve` 后，在浏览器访问 **http://127.0.0.1:3030/** 即可：
- 查看钱包 SOL / USDC 余额
- 查看 DCA 收益统计（买入次数、累计投入、浮动盈亏等）
- 点击「立即买入」按配置金额执行一次 USDC → SOL
- 查看最近买入记录（时间、金额、交易签名）
- 页面每 30 秒自动刷新余额与统计

## 项目结构

```
src/
  lib.rs      # 模块入口
  config.rs   # 配置与常量（USDC/SOL mint、默认路径等）
  wallet.rs   # 从 JSON 加载 keypair
  rpc.rs      # Solana RPC 封装（余额、发交易）
  swap.rs     # Jupiter quote + swap，签名并发送
  pnl.rs      # 买入记录存储与收益计算
  web.rs      # Web 服务与 API（/api/balance, /api/stats, /api/buy 等）
  main.rs     # CLI（run / buy / stats / serve）
static/
  index.html  # Web 界面单页（内联 CSS/JS，编译进二进制）
```

## 注意事项

- 主网交易会消耗 SOL 作为 gas，请保证钱包有少量 SOL。
- 若出现 **"Attempt to debit an account but found no record of a prior credit"**：多为**输入代币（如 USDC）余额不足**或**从未收过该代币（代币账户未创建）**，或 **SOL 不足无法支付 gas**。请先转入足够 USDC 与少量 SOL 再试。
- 收益统计中的「当前价」目前用**最近一笔买入价**近似；如需实时市价可后续接入 Jupiter Price API 或其它行情源。
- 请妥善保管 keypair，不要提交到仓库或暴露给他人。
- Web 界面仅监听本机（0.0.0.0），如需外网访问请自行加反向代理与鉴权。

## License

MIT
