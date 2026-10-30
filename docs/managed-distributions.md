# CC Switch 受管发行包

本适配器实现 Linux x86_64/aarch64 的 CC Switch AppImage 安装、更新、按变更回执恢复、卸载和显式启动。它操作 LMM 自己创建的私有目录。受管发行包、应用配置、独立应用授权、当前模型、实际调用与付费验收分别报告。

## 固定来源

当前固定版本为 **4.0.22**，源码提交 `b76426741ef93069e2f374ac01ca7ef5760e1d0e`，原生数据库版本 **24**。

| 平台 | 官方文件 | 字节数 | SHA-256 |
| --- | --- | ---: | --- |
| Linux x86_64 | CC-Switch-v4.0.22-Linux-x86_64.AppImage | 97339896 | 9090a1bc5f58ccbc72444302c1a3d8001dd1b00fbff7b0d6b64bd1be58077313 |
| Linux aarch64 | CC-Switch-v4.0.22-Linux-arm64.AppImage | 94763528 | b00ffbe004fe7885ddc7fa33badb965a2cf4d626a259287ea9a4f89fa78bc66e |

来源：[官方发布](https://github.com/farion1231/cc-switch/releases/tag/v4.0.22)、[固定提交](https://github.com/farion1231/cc-switch/commit/b76426741ef93069e2f374ac01ca7ef5760e1d0e)、[数据库版本](https://github.com/farion1231/cc-switch/blob/b76426741ef93069e2f374ac01ca7ef5760e1d0e/src-tauri/src/database/mod.rs)、[表结构](https://github.com/farion1231/cc-switch/blob/b76426741ef93069e2f374ac01ca7ef5760e1d0e/src-tauri/src/database/schema.rs)、[供应商读写](https://github.com/farion1231/cc-switch/blob/b76426741ef93069e2f374ac01ca7ef5760e1d0e/src-tauri/src/database/dao/providers.rs)。

下载限于固定 HTTPS 官方发布 URL，重定向限于 GitHub 发行资产域名，必须同时匹配文件长度与 SHA-256。此机制没有声称完成上游 `.sig` 签名验证。更新目标随 CLI 的已审阅固定发布记录演进，不把网络上的“latest”直接当作可以执行的新程序。

## 计划与执行

```bash
lmm install cc-switch --dry-run --json
# 审阅上一步的版本、路径、来源和 plan_id，然后使用原值：
lmm install cc-switch --yes --plan-id <plan_id> --json
lmm status cc-switch --json
lmm run cc-switch --json

lmm update cc-switch --dry-run --json
lmm update cc-switch --yes --plan-id <plan_id> --json

lmm uninstall cc-switch --dry-run --json
lmm uninstall cc-switch --yes --plan-id <plan_id> --json
```

`--dry-run` 完全只读。交互终端会先显示完整计划，再请求确认；无人值守执行需要 `--yes --plan-id`。旧计划在受管状态改变后失效。`--yes` 单独使用只展示待确认计划。安装不需要 LMM 登录。`update --all` 当前只检查已由 LMM 管理的 CC Switch；没有受管安装时会拒绝，不把软件目录当成安装清单。

默认程序目录为 `$XDG_STATE_HOME/lmm/software/cc-switch`，或 `~/.local/state/lmm/software/cc-switch`。用 `--managed-root /absolute/private/path` 明确指定另一个 LMM 管理目录。`--instance` 指向 CC Switch 的应用数据目录，其作用是原生数据库检查和版本兼容门禁，不改变应用启动时的原生实例设置。`run --instance` 会拒绝，避免启动另一个未确认的实例。

安装不修改 PATH、系统包管理器、桌面快捷方式或应用配置；使用 `lmm run cc-switch` 启动受管程序。AppImage 的图形环境和 FUSE 依赖由本机提供。`process_started` 仅证明已创建子进程，不能作为应用健康、联网或实际调用通过的证据。

## 回退、取消与未知结果

`status --json` 的 `managed_installation.history` 保留 `change_id` 和原 `plan_id`。恢复必须指定一个变更，只有当前受管发行包仍等于该变更写入的值时才会撤销；当前已经等于原值则保持不变。后续不同的用户选择产生冲突。

```bash
lmm restore cc-switch --change <change_id> --dry-run --json
lmm restore cc-switch --change <change_id> --yes --plan-id <plan_id> --json
```

更新保留经过验证的旧发行包供回退。回退不会回滚数据库：如果原生 `user_version` 已超过旧程序支持的版本，会拒绝回退。原生 SQLite 写事务保护本地版本提交期间的 schema 检查，第三方数据没有被修改。

安装先持久写入 `pending` 计划，再下载或删除文件，最后原子提交当前发行包和回执。下载失败或进程退出后，用相同参数与原 `plan_id` 续作。提交成功但响应丢失时，同一原计划返回历史回执，后续更新不会被重新执行的旧计划覆盖。

在原安装仍完整时，可取消 pending 计划，例如：

```bash
lmm update cc-switch --yes --plan-id <original_plan_id> --cancel-pending --json
```

取消会保存回执、递增代次，并永久拒绝延迟到达的原计划。若卸载已经删除部分受管文件，取消会明确拒绝；应继续原卸载操作，而不能报告旧安装仍完整。

受管目录使用 OS 文件锁，并拒绝符号链接、非私有目录和未知归属。文件发布不覆盖既有路径。操作只移除日志中已登记且仍匹配 SHA-256 的发行包。后续修改的程序文件、其他用户文件和 CC Switch 数据会保留。卸载保留不含凭据的变更日志，便于查回执。

## 原生数据库检查与剩余接入边界

```bash
lmm status cc-switch --instance /absolute/path/to/.cc-switch --json
```

显式实例检查使用只读 SQLite 事务，要求 schema 24 和已核验的 `providers` 主键与列。仅查询 ID、应用类型、名称及数据库选择标志；不读取 `settings_config`、`meta` 或凭据内容。`selected_in_database` 不能证明本机 Codex/Claude 配置已经激活，也不能证明经过本地代理。

`setup`、`sync` 和 `disconnect` 的完整应用接入仍需要独立应用授权与原生配置激活合同。现有 CLI 登录只拥有 `catalog:read balance:read`，不得把该 token 写进 CC Switch 的供应商配置。不得只修改 `is_current` 就声称完成激活，因为上游激活还写本机应用配置。这些尚未具备的流程继续返回明确阻塞，不返回空成功。

隔离测试使用合成发行包字节、临时目录和真实 SQLite；不下载或执行官方 AppImage，不访问真实账号或支付渠道。真实 Linux 图形环境运行、官方文件端到端下载与模型调用仍需独立验收。
