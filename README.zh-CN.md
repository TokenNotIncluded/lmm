<div align="center">

# lmm

**在终端使用 Language Model Mesh。**

查询服务商预设、检查本地工具、读取 LMM 模型目录。

[![Rust](https://img.shields.io/badge/Rust-1.88%2B-000000?style=flat-square&logo=rust)](Cargo.toml)
[![CLI](https://img.shields.io/badge/CLI-lmm-2563eb?style=flat-square)](#开始使用)
[![Languages](https://img.shields.io/badge/languages-English%20%7C%20简体中文-0f766e?style=flat-square)](#语言)
[![License](https://img.shields.io/badge/license-AGPL--3.0-64748b?style=flat-square)](LICENSE)

[English](README.md) · **简体中文**

[开始使用](#开始使用) · [命令](#命令) · [使用指南](docs/usage.zh-CN.md) · [贡献翻译](docs/i18n.md)

</div>

---

## 开始使用

使用 `mise.toml` 固定的开发工具，从当前源码构建：

```sh
git clone https://github.com/TokenNotIncluded/lmm.git
cd lmm
mise install
mise exec -- cargo install --path . --locked

lmm --help
lmm presets deepseek --json
lmm catalog
```

Cargo 包名为 **`lmm-cli`**，命令名为 **`lmm`**。本仓库独立构建，不需要先启动 LMM 后端。源码构建需要 Rust 和本机编译工具；Linux 的 OAuth 登录还需要运行中的 Secret Service。

## 命令

| 用途 | 命令 | 实际行为 |
| --- | --- | --- |
| 查询服务商 | `lmm presets [关键词]` | 离线搜索内置 Magpie 预设快照 |
| 查询软件 | `lmm catalog [关键词]` | 搜索项目名称和用途说明 |
| 检查软件 | `lmm status astrbot --instance /绝对路径` | 检查本地路径线索，不读取配置内容 |
| 分享诊断 | `lmm doctor astrbot --report --json` | 输出未完成的诊断摘要，并移除本地路径 |
| 读取账号模型 | `lmm login` → `lmm models --json` | 授权 CLI，再读取模型与价格，不调用模型 |
| 预览接入 | `lmm setup astrbot --dry-run --json` | 列出计划和阻塞原因，不修改软件或配置 |

**当前为开发预览。** 安装、接入写入、同步、更新、恢复、解除接入、卸载和启动尚未实现。这些命令会返回阻塞原因，不会把空操作报告为成功。目录条目不代表软件已经通过兼容性验证。

`presets` 是服务商预设，`catalog` 是软件目录，`models` 是当前账号获准读取的模型。三者不混用。

## 语言

英文和简体中文随程序一起打包，不需要联网下载翻译，也不需要额外放置语言文件。

```sh
lmm --lang en --help
lmm status astrbot --lang zh-CN
LMM_LANG=zh-CN lmm catalog
```

选择顺序：**`--lang` → `LMM_LANG` → `LC_ALL` → `LC_MESSAGES` → `LANG` → 英文**。

环境变量支持 `zh_CN.UTF-8`、`zh-Hans` 等常见写法。空值会被忽略，不支持的语言使用英文。`LC_ALL=C` 使用英文，但可被 `LMM_LANG` 或 `--lang` 覆盖。

JSON 字段名、状态值、标识符、价格和时间戳不随语言改变；面向人的说明和消息会翻译。服务商原名和上游数据保持原样。

## 登录

```sh
lmm login
lmm models --json
lmm logout
```

CLI 只申请**模型目录和余额读取权限**，不申请模型调用权限，也不替其他应用授权。凭据存入系统凭据库，不会退回到明文文件。

浏览器必须能访问 CLI 所在机器的回环端口。`--no-browser` 只显示链接，**不是设备码登录**。认证命令暂不支持 `--non-interactive`；即使使用 `models --json`，系统仍可能要求解锁凭据库。

自托管地址、回调等待时间、凭据保存和撤销行为见[使用指南](docs/usage.zh-CN.md)。

## 开发

```sh
python3 scripts/check-i18n.py
python3 scripts/test_i18n_checks.py
mise exec -- cargo fmt --all --check
mise exec -- cargo test --locked
mise exec -- cargo clippy --locked --all-targets --all-features -- -D warnings
mise exec -- cargo build --locked --release
```

[贡献指南](AGENTS.md) · [翻译指南](docs/i18n.md) · [产品需求](docs/requirements.zh-CN.md) · [Magpie 功能对照](docs/magpie-parity.md)

## 退出码

| 退出码 | 含义 |
| --- | --- |
| `0` | 查询已完成；仍需检查各层状态，不能据此认定软件可用 |
| `2` | 参数不完整或无效、目标未知、选择失败或输出失败 |
| `3` | 操作被阻止、能力未实现或诊断未完成 |

## 来源与许可证

项目使用 [AGPL-3.0-only](LICENSE)。服务商预设来自固定版本的 [Magpie](third_party/magpie/NOTICE.md)，保留其 MIT 许可与来源说明。LMM 使用 Rust 实现，不包装或启动 Magpie。详见 [NOTICE](NOTICE)。
