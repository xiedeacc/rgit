# rgit 部署手册（NAS）

布局与流程详见 [DESIGN.md](DESIGN.md) §3/§12/§14。

## 1. 构建 + 安装

```bash
# 在构建机（或 NAS 本机，需 rust + flutter）
cd /root/src/rust/rgit
sudo scripts/deploy.sh
```

`deploy.sh` 完成：release 构建（rgit / rgit-migrate）、`flutter build web`、
铺设 `/opt/usr/local/rgit/{bin,conf,data,logs}`、创建 `git` 运行用户、
安装 `rgit.service` + `rgit-backup.service` + `rgit-backup.timer` 并启用。

跨机部署：在构建机跑 `cargo build --release` 与 `flutter build web` 后，
rsync `target/release/rgit*`、`web/build/web`、`scripts/rgit-backup.sh`
到 NAS 对应目录，再手工安装 systemd 单元（模板在 deploy.sh 内）。

## 2. 配置

```bash
sudo vi /opt/usr/local/rgit/conf/rgit.toml   # 必改 external_url / clone_host
sudo chmod 0600 /opt/usr/local/rgit/conf/rgit.toml
```

首次启动：

- 数据库/存储目录自动创建，SSH host key 自动生成
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

## 4. 备份

- 部署脚本已启用 `rgit-backup.timer`（每小时，`Persistent=true`）
- 前置：给运行用户生成 SSH key 并加为 GitHub `xiedeacc/rgit_data` 仓库的
  deploy key（写权限）：

```bash
sudo -u git ssh-keygen -t ed25519 -N '' -f /opt/usr/local/rgit/.ssh/id_ed25519
sudo -u git ssh -T git@github.com   # 首次确认 known_hosts
sudo systemctl start rgit-backup.service   # 手动跑一次验证
```

- 恢复：`bin/rgit-restore [目标目录]`（clone 备份仓库 → 重组分块大文件 →
  拷回 → 用快照恢复 rgit.db）

## 5. GitLab 数据迁移

见 [MIGRATION.md](MIGRATION.md)。顺序：先完成本手册 1–2 部署（不启动服务或
先停掉），再跑 rgit-migrate 落库到 `data/`，最后启动 `rgit.service`。

## 6. 冒烟验收

```bash
curl -sk https://git.example.com/api/v1/projects | head       # API 可达
git clone https://git.example.com/root/demo.git               # HTTP clone
git clone ssh://git@git.example.com:2222/root/demo.git        # SSH clone
cd demo && date > f && git add f && git commit -m t && git push
git lfs track "*.bin" && dd if=/dev/urandom of=big.bin bs=1M count=5 \
  && git add . && git commit -m lfs && git push                # LFS 上传
```
