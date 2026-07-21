# rgit 部署手册（NAS）

布局与流程详见 [DESIGN.md](DESIGN.md) §3/§12/§14。

## 1. 构建 + 安装

```bash
# 在 dev 构建机运行；脚本会通过 ssh/rsync 发布到 NAS
cd /root/src/rust/rgit
scripts/deploy.sh
```

`deploy.sh` 是唯一生产发布入口，完成：

- dev 上 release 构建 `rgit` / `rgit-shell` / `rgit-migrate`
- dev 上 `flutter build web` 并注入当前 commit 信息
- 在 NAS 创建 `/tmp/rgit-deploy-<rev>` staging 目录
- 用 `rsync` 上传二进制、Web bundle、辅助脚本、`conf/rgit.example.toml`
  和 `scripts/systemd/` 下的 systemd unit
- 在 NAS 安装到 `/opt/usr/local/rgit/{bin,conf,data,logs}`，上传 unit 到
  `/etc/systemd/system`，enable timer，重启 `rgit.service`
- 验证线上 `main.dart.js` 包含本次 commit，且 `rgit.service` active、
  `gitlab-runsvdir.service` inactive

常用覆盖项：

```bash
RGIT_DEPLOY_HOST=nas \
RGIT_VERIFY_URL=https://rgit.xiedeacc.com \
scripts/deploy.sh
```

发布前可运行 `scripts/test-deploy.sh`，它验收 release 产物和
`scripts/systemd/` unit 语法，不会连接 NAS，也不会写 `/etc`。
依赖安全检查使用 `scripts/security-audit.sh`；脚本同时验证 SQLx 锁文件中的
MySQL/RSA 可选依赖不在任何工作区目标的依赖图中。

## 2. 配置

```bash
sudo vi /opt/usr/local/rgit/conf/rgit.toml   # 必改 external_url / clone_host
sudo chmod 0600 /opt/usr/local/rgit/conf/rgit.toml
```

### 2.1 OpenSSH Git 入口

生产配置使用 `[ssh] mode = "system"`。系统 `sshd` 继续监听 Git 端口，rgit 从
SQLite 原子重建 `authorized_keys_file`，每一行以 forced command 调用
`rgit-shell --key-id ...`。`rgit-shell` 仅接受 upload-pack、receive-pack、
upload-archive 与 git-lfs-authenticate，不提供交互 shell、PTY 或端口转发。

让 sshd 的 `git` 用户读取配置中的 `authorized_keys_file`；若它不是
`git` 用户 home 下的默认 `.ssh/authorized_keys`，增加 Match 配置：

```text
AcceptEnv GIT_PROTOCOL
Match User git
    AuthorizedKeysFile /opt/usr/local/rgit/data/ssh/authorized_keys
```

执行 `sshd -t` 后 reload。首次启动 rgit、迁移后启动以及 SSH key API
增删都会重建该文件。不要同时让 GitLab 与 rgit 写同一个 authorized_keys。

空实例首次启动：

- 数据库/存储目录自动创建；system SSH 模式复用系统 sshd host key
- 若用户表为空，自动创建管理员 `root`：
  密码取 `RGIT_INITIAL_ROOT_PASSWORD` 环境变量，未设置则随机生成并打印在日志
  （`logs/rgit.log` 搜 "initial admin"），首次登录后立即修改

## 3. nginx

```bash
sudo cp conf/nginx/rgit.conf /etc/nginx/conf.d/
sudo vi /etc/nginx/conf.d/rgit.conf   # 域名与证书
sudo nginx -t && sudo systemctl reload nginx
```

关键项（缺一 push/LFS 会挂）：`client_max_body_size 0`、
`proxy_request_buffering off`、`proxy_buffering off`、超时 3600s。

若证书含 Must-Staple（`openssl x509 -text` 显示
`TLS Feature: status_request`），reload 前必须准备 `ssl_stapling_file`。部署包中的
`bin/rgit-refresh-ocsp` 会下载并验证 OCSP 响应、原子替换文件，并在 `nginx -t`
通过后 reload；NAS 应用 systemd timer 每 12 小时刷新。这样 Git/GnuTLS 客户端
不会在 Nginx reload 后的异步 OCSP 获取窗口内偶发证书校验失败。

## 4. 备份

- 部署脚本已 enable `rgit-backup.timer`（每小时，`Persistent=true`）；SSH deploy
  key 配好并完成首次迁移后再 `systemctl start rgit-backup.timer`
- 前置：给运行用户生成 SSH key 并加为 GitHub `xiedeacc/rgit_data` 仓库的
  deploy key（写权限）。systemd unit 显式指定 `/opt/usr/local/rgit/.ssh` 下的
  key 与 known_hosts，不会误用 NAS 上原 GitLab `git` 用户的
  `/var/opt/gitlab/.ssh`：

```bash
sudo -u git ssh-keygen -t ed25519 -N '' -f /opt/usr/local/rgit/.ssh/id_ed25519
sudo -u git ssh -i /opt/usr/local/rgit/.ssh/id_ed25519 -o IdentitiesOnly=yes \
  -o UserKnownHostsFile=/opt/usr/local/rgit/.ssh/known_hosts -T git@github.com
sudo systemctl start rgit-backup.service   # 手动跑一次验证
```

- 仿照 AWS rblog，把 `/opt/usr/local/rgit/{bin,conf,data}` 镜像到部署目录内的
  `.backup-worktree`；`logs/`、`.ssh/`、`repositories/`、`lfs-objects/` 和外置
  `/zfs/gitlab_data` 始终排除
- 恢复：`bin/rgit-restore [目标目录]`（clone 备份仓库 → 重组分块大文件 →
  恢复 `bin/conf/data` → 校验 SQLite）；真实 Git 项目和 LFS
  需要由共享存储自身的快照/灾备策略恢复

## 5. GitLab 数据迁移

见 [MIGRATION.md](MIGRATION.md)。顺序：先完成本手册 1–2（保持服务未启动），
确认 `data/` 为空，再跑 rgit-migrate，最后启动 `rgit.service` 和备份 timer。

## 6. 冒烟验收

```bash
curl -sk https://git.example.com/api/v1/projects | head       # API 可达
git clone https://git.example.com/root/demo.git               # HTTP clone
git clone ssh://git@git.example.com:10022/root/demo.git       # SSH clone
cd demo && date > f && git add f && git commit -m t && git push
git lfs track "*.bin" && dd if=/dev/urandom of=big.bin bs=1M count=5 \
  && git add . && git commit -m lfs && git push                # LFS 上传
```
