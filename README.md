# lmm

**LMM API · 连接模型与工具，让智能自由流动**

LMM = **Language Model Mesh**，中文「语言模型网络」。Mesh 表达互联与协作，呼应多模型路由、渠道共享和微服务协作的方向。

这是 [TokenNotIncluded/lmm](https://github.com/TokenNotIncluded/lmm) 独立维护的 Rust CLI。crates.io 包名为 `lmm-cli`，可执行文件名为 `lmm`；父仓库通过子模块锁定版本，CLI 的构建和依赖独立管理。

**当前是开发预览，已实现服务商预设查询、只读发现、桌面 OAuth 登录和模型目录读取，不是可完成应用接入的产品版本。**
除下文明确限定的 Pi Provider 适配器外，`setup`、安装、同步、更新、恢复、解除接入、卸载和启动会明确返回阻塞原因，
不会以空操作返回成功。没有修改第三方软件或生产服务。

完整产品要求见 [原始需求](docs/requirements.zh-CN.md)。完整需求保留为目标，不因当前实现范围而缩减。
与 Magpie 的功能对照及下一步任务见 [Magpie 对照](docs/magpie-parity.md)。实现使用 Rust，不包装或启动 Magpie，也不依赖 Go 运行时。

## 从 crates.io 安装

```bash
cargo install lmm-cli --version 0.1.0 --locked
lmm --help
```

服务商预设查询是本仓库的新实现，本轮尚未发布；体验这项功能请使用下文的源码构建命令。

这是开发预览版，功能边界见上文。Cargo 安装需要 Rust 1.88 或更新版本及本机编译工具；
面向无需开发环境用户的二进制安装器尚未提供。
Linux OAuth 功能另需运行中的 Secret Service；Windows/macOS 使用系统凭据库。

## 当前可用

- `lmm presets [关键词] [--json]`：查询 56 项内置服务商预设，离线运行，不读取密钥、不访问服务商。
- `lmm catalog [关键词]`：搜索明确项目身份的软件目录，分别说明安装、接入、维护能力。
- `lmm status [软件]`：当前进程 OS/架构、WSL/容器/SSH 线索、PATH 候选文件及显式实例路径检查。
- `lmm doctor [软件] --report`：只读诊断摘要，移除实例和文件路径，不读取配置内容。
- `lmm setup [软件] --dry-run`：生成带阻塞原因的预览；无目标且有终端时可选择软件。
- `--json`：结构化结果；自动禁用交互。`--non-interactive` 同样禁止等待输入。
- `lmm login`：桌面浏览器 PKCE 登录；凭据保存于系统凭据库。
- `lmm models [--json]`：获取当前授权模型及价格，不调用模型、不切换配置。
- `lmm balance [--json]`：读取余额 V2，保留服务端整数积分及美元单位；未知单位或失败不会显示为零。
- `lmm logout`：撤销当前 issuer 下的 CLI 令牌族，成功后删除本地凭据；不影响 Pi/DSH 等其他应用。

## Pi Provider 适配器（Linux / Pi 1.2.0）

此适配器连接**已经安装的官方 Pi 1.2.0 npm 包**，仅管理明确项目内的 LMM Provider。
它不安装或卸载 Pi 本体，不复制 CLI 登录凭据。Pi 使用自己的 `/login` 授权、`/model` 选择和原生模型调用。
固定 Provider 来源为 `git:github.com/TokenNotIncluded/pi-lmm-provider@f0c54764eb3d73e50790346f0dbab834587fe27b`。

```bash
lmm setup pi --add-only --instance /absolute/project --node /absolute/node \
  --pi-package /absolute/node_modules/@earendil-works/pi-coding-agent \
  --agent-dir /absolute/pi-agent --dry-run --json
# 核对预览后，用相同参数移除 --dry-run，并添加 --yes
```

`setup` / `install` 只添加固定 Provider；结果为 `provider_added_authorization_pending`，不代表模型已可用。
`setup`、`sync`、`disconnect` 的后续授权/目录刷新/撤销仍需目标 Pi 完成，因此实际执行返回 3 并报告具体阶段。
已有其他 LMM 安装来源时拒绝创建副本；默认服务商和模型保留当前选择。`--activate` 必须交由目标 Pi 的交互授权和模型选择完成。
`update` 只接受仍存在的自有固定来源，本版本不自动追逐最新版。
`sync` 验证 Provider 存在，明确返回 `catalog_refresh_pending`；在 Pi 中运行 `/lmm-prices` 才会刷新目录。
`run pi` 使用相同的四个显式路径启动 Pi，只允许交互终端。

`restore` / `disconnect` 加相同目标路径和 `--yes`，仅从原生设置事务中移除本次添加的精确来源。
这不会撤销 Pi OAuth 授权；需要撤销时在 Pi 中运行 `/lmm-revoke`。
用户后来新增的插件、默认模型及其他字段都会保留；若用户改写了此来源对象则拒绝恢复。
下载代码可能被其他实例引用，因此保留。`uninstall` 仍受阻，不删除 Pi 本体或未知资源。

无秘密日志在所选项目 `.pi/lmm-owned.json`，锁在 `.pi/lmm-adapter.lock`。
修改前持久化 pending，读回成功后才完成日志；中断或不确定结果保持待检查状态，不自动声称完成。
配置增删在 Pi 自己的 `FileSettingsStorage.withLock` 中读取最新内容执行，避免用旧插件数组覆盖并发编辑。
普通 CLI 的 OAuth 凭据继续只保存在系统凭据库；适配器从不读取 Pi 的凭据文件。

## 服务商预设

```bash
lmm presets
lmm presets deepseek --json
```

预设来自 [Magpie 的固定版本](https://github.com/yetone/magpie/blob/d5cae267a727e23f4b6e2b28aa107ecdeac5aa9c/internal/provider/presets.go)，是服务商地址及相关元数据的静态目录。它不是已经配置的服务商列表，不代表当前账号有权限或额度，也不代表已完成服务商接入、模型发现或调用验证。端点和模型信息可能随服务商变化，不能作为实时承诺。

`presets` 查询服务商预设，`catalog` 查询软件目录，`models` 读取 LMM 账号的授权模型；三者分别表示不同内容。

## 登录与模型目录

```bash
lmm login
lmm models --json
lmm balance --json
lmm logout
# 自托管必须明确指定可信的 HTTPS origin，认证和只读命令使用同一 issuer
lmm login --issuer https://lmm.example.com --no-browser
```

服务端需要注册 `lmm` 客户端并启用已有 OAuth 服务。使用前应确认目标服务支持这些接口，并验证实际账号的浏览器登录流程。
CLI 只申请 `catalog:read balance:read` 和同意页显示的分组快照；没有 `models:invoke`、
MCP 或账号管理权限。CLI 登录成功不代表 AstrBot 或 CC Switch 已授权。

浏览器回调只监听 `127.0.0.1` 随机端口，等待默认 180 秒，可用 `--timeout 10..600` 调整。
`--no-browser` 仅关闭自动打开浏览器，**不是设备码登录**：浏览器仍须能访问本机回环端口。
无图形服务器跨设备授权尚未实现；非交互 `login` 直接返回，不暗中等待授权。
切换账号先 `logout` 再 `login`；已有 CLI 登录不会被新登录静默覆盖。

Windows 使用 Credential Manager，macOS 使用 Keychain，Linux 使用 Secret Service。
凭据库可能显示系统解锁提示；不可用时明确报错，不使用明文文件后备。
因此认证命令暂不支持 `--non-interactive`，会在访问凭据库之前退出；`models --json`、`balance --json` 只禁用 CLI 文本交互，系统仍可能要求解锁。
源码构建将 Linux D-Bus 库静态编入，
运行时仍需要可用的 Secret Service；不能将开发机单测当作三个系统的实际凭据库验收。

认证操作使用当前用户状态目录中的无密钥 `oauth.lock`，防止并发登录/刷新/退出。
Linux 为 `$XDG_STATE_HOME/lmm` 或 `~/.local/state/lmm`，macOS 为
`~/Library/Application Support/lmm`，Windows 为 `%LOCALAPPDATA%/lmm`。
Unix 目录/锁文件新建权限分别为 0700/0600；已有不安全目录会被拒绝。
刷新前将“不可重试”状态存入凭据库，响应丢失或保存失败后不会自动重放旧 refresh token。
此时需要退出再登录；退出撤销失败保留本地记录供重试，不能报告已完成。

模型目录只表示账号可见范围，不表示某个软件已经兼容。未知价格保留 null，
价格包含服务端返回的倍率，不重复计算；本命令不执行付费测试。

## 本地软件检查

显式实例目前只接受 AstrBot 部署根目录、CC Switch 配置根目录：

```bash
lmm status astrbot --instance /absolute/path/to/AstrBot
lmm status cc-switch --instance /absolute/path/to/.cc-switch
lmm doctor astrbot --instance /absolute/path/to/AstrBot --report --json
lmm setup astrbot --dry-run --json
```

Windows 使用当前 Windows 环境的绝对路径。AstrBot 只检查所选根目录的
`data/cmd_config.json` 路径；CC Switch 检查 `cc-switch.db` 路径，不打开数据库。
不自动搜索项目、其他用户、Docker 实例、局域网或远程机器。
PATH 文件可能是包装脚本或来自其他环境，因此只标为线索；不执行 `--version`。
符号链接和不可访问项显示 `unknown`，文件缺失只描述被检查的那个位置。

目前所有软件目录条目的兼容版本/平台/场景列表为空，均未验证。
配置管理者、授权状态、实际服务商、模型及真实调用路径不会凭路径推断。
`status` 的退出码 0 只表示完成线索查询，不能用于判断“软件可使用”。
`doctor` 尚不能完成语义检查，会返回 3。`status`、`doctor`、`setup --dry-run` 没有网络调用或付费验证。

`--all` 不把目录视为安装列表；当前没有合格适配器，因此返回空计划与退出码 3。
`--yes` 不能消除能力缺失，不能代表支付、重新授权或切换服务商的同意。
单独 `install` 的计划不要求登录。

## 开发

从这个独立仓库的根目录执行，工具版本由 `mise.toml` 固定：

```bash
mise install
mise exec -- cargo run --locked -- presets deepseek --json
mise exec -- cargo run --locked -- catalog
mise exec -- cargo test --locked
mise exec -- cargo clippy --locked --all-targets --all-features -- -D warnings
mise exec -- cargo fmt --all --check
mise exec -- cargo build --locked --release
```

不需要先构建或启动父仓库中的核心、扩展或 Web 服务。独立 Cargo 工程与后端分开锁定依赖，避免将 CLI 发布绑定到后端构建。
源码开发需要 Rust；面向普通用户的签名二进制、安装器和更新渠道尚待发布，
当前没有声称普通用户已经能“一键安装”。CI 构建产物仅作开发预览，不自动发布。

## 退出码与安全边界

| 退出码 | 含义 |
| --- | --- |
| 0 | 目录/只读查询已输出；请继续检查各层状态 |
| 2 | 参数不完整、目标未知、无法选择目标或输出失败 |
| 3 | 能力未实现、检查未完成或操作被阻止 |

`restore_fields` 是内存中的三方字段恢复算法：仅在字段仍等于 LMM 写入值时撤销，
保留用户后续修改；任一冲突使整个目标的计算失败。它拒绝整对象、数组及数组索引恢复。
它没有连接文件写入、日志、授权或命令执行，不能当作 `lmm restore` 已完成的证据。
同样，付费检查函数只是客户端规划规则，**不能代替服务端消费限制**。

指纹函数只用于有界配置快照的内容比较，不是原子写锁。未来接入必须使用目标软件原生
事务/管理接口解决并发写入；“比较哈希后覆盖文件”不满足完整并发修改验收。

只读环境查询不建立状态目录；OAuth 操作会创建上述锁目录并使用系统凭据库。
LMM 不复制第三方凭据、不上传遥测、不进行付费调用。
分享前建议使用 `doctor --report --json`；普通 `status` 会显示本地路径。

## 来源与许可证

项目按 [AGPL-3.0-only](LICENSE) 发布。服务商预设参考 Magpie 的 MIT 许可源码；来源及保留的许可说明见 [NOTICE](NOTICE)。参考预设数据不意味着具备 Magpie 的全部能力。
