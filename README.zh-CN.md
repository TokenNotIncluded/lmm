# LMM CLI

[English](README.md) · [使用指南](docs/usage.zh-CN.md) · [翻译指南](docs/i18n.md)

LMM（Language Model Mesh，语言模型网络）是独立的 Rust CLI，用于服务商预设查询、本地软件检查、账号模型目录与余额读取，以及范围明确的应用适配器。Cargo 包名为 `lmm-cli`，可执行文件为 `lmm`；构建和依赖锁文件独立于父项目服务。

当前源码是开发预览，包含显式安装的 Pi 1.2.0 SDK 的 Linux 适配器，以及 CC Switch 受管发行包适配器。计划、数据库写入或进程启动成功，均不能证明应用已通过 LMM 路由激活或模型调用可用。详细边界见[使用指南](docs/usage.zh-CN.md)、[受管发行包合同](docs/managed-distributions.md)和[原生接入证据](docs/native-credential-evidence.md)。

## 构建当前源码

```sh
mise install
mise exec -- cargo install --path . --locked
lmm --lang zh-CN --help
```

使用本仓库 `mise.toml` 固定的开发工具链（Rust 1.100.0）；Cargo 清单声明的最低版本为 Rust 1.99.1。构建需要本机编译工具。Linux OAuth 另需运行中的 Secret Service，Windows 和 macOS 使用系统凭据库。已发布包可能不同于当前源码；这里尚未提供面向无开发环境用户的独立安装器。

## 选择语言

```sh
lmm --lang en catalog astrbot
lmm catalog astrbot --lang zh-CN
LMM_LANG=zh-CN lmm --help
```

全局 `--lang` 优先于 `LMM_LANG`、`LC_ALL`、`LC_MESSAGES`、`LANG` 中首个非空值。未知环境语言回退英文；不支持的显式参数会被拒绝。帮助和终端文字支持中英文；JSON 的目录用途、提示和已识别错误正文在两种语言下均保持固定英文。命令名、标识符、协议值和数字不变，详见[翻译指南](docs/i18n.md)。

## 当前命令

| 任务 | 入口 | 结果与边界 |
| --- | --- | --- |
| 服务商预设 | `lmm presets [query] --json` | 离线的 56 项预设快照；不配置服务商、不验证连通性 |
| 软件目录 | `lmm catalog [query]` | 项目身份，以及分别列出的安装、接入、维护能力 |
| 本地检查 | `lmm status [software] --json` | 路径线索、CC Switch 受管状态或显式选择的原生设置 |
| 分享诊断 | `lmm doctor [software] --report --json` | 移除本地路径，明确保留未完成的验证项 |
| CLI 账号 | `lmm login`、`models`、`balance`、`logout` | 浏览器 PKCE 和账号只读权限；不授予应用模型调用权限 |
| CC Switch 受管程序 | `lmm install cc-switch --dry-run --json` | 固定官方版本计划；执行时需要精确 `plan_id` |
| CC Switch 原生配置 | `lmm setup cc-switch --help` | 独立应用授权和仅添加的原生数据库事务；激活另行处理 |
| Pi Provider | `lmm setup pi --help` | 显式指定 Node、Pi 1.2.0 SDK、项目和 agent 路径；授权保留在 Pi 内 |

`--json` 禁用 CLI 提示；`--non-interactive` 不等待缺失输入。可能需要系统解锁的凭据操作会在访问凭据前拒绝无人值守模式。`--yes` 不能替代缺失的适配器、必要的计划 ID、应用同意或服务端消费上限。

## 从只读命令开始

```sh
lmm presets deepseek --json
lmm catalog
lmm status astrbot --instance /absolute/path/to/AstrBot --json
lmm doctor astrbot --instance /absolute/path/to/AstrBot --report --json
lmm install cc-switch --dry-run --json
```

显式路径属于运行 CLI 的当前操作系统。CLI 不扫描其他用户、远程系统或容器实例。请求写入之前，先阅读[使用指南](docs/usage.zh-CN.md)中的 Pi 设置检查、CC Switch 回执、OAuth、恢复和退出码说明。

## 开发与验证

```sh
python3 scripts/check-i18n.py
python3 scripts/test_i18n_checks.py
mise exec -- cargo fmt --all -- --check
mise exec -- cargo test --locked
mise exec -- cargo clippy --locked --all-targets --all-features -- -D warnings
```

测试使用隔离 fixture。两项 Linux 凭据迁移检查需要显式启用并提供隔离的 Secret Service；普通测试不能代替各操作系统真实凭据库验收。本地验证不会发布 crate、正式版本或生产部署。

## 范围与来源

[原始需求](docs/requirements.zh-CN.md)继续作为产品目标；[Magpie 对照](docs/magpie-parity.md)基于固定上游版本记录已实现能力与剩余工作。LMM 将行为移植到 Rust，不包装或启动 Magpie，也不依赖 Go 运行时。

项目使用 [AGPL-3.0-only](LICENSE) 许可。内置服务商快照参考 MIT 许可的 Magpie 材料，见 [NOTICE](NOTICE) 和[第三方来源记录](third_party/magpie/NOTICE.md)。静态预设不代表当前账号有权限，也不代表运行时兼容。
