# GitLab → rgit 迁移手册

目标：把 NAS 上 omnibus GitLab（17.11.x）的数据迁到 rgit，**不丢任何 git 项目**。
设计依据见 [DESIGN.md](DESIGN.md) §13。整个过程只读 GitLab 数据，可随时回退。

## 0. 前提检查

```bash
# GitLab 版本（建议 17.x）
sudo gitlab-rake gitlab:env:info | head

# 所有项目必须已是 hashed storage（GitLab 13+ 默认；有 legacy 的先迁）
sudo gitlab-rails runner 'puts Project.where("coalesce(storage_version,0) < 2").count'
# 若 > 0：sudo gitlab-rake gitlab:storage:migrate_to_hashed 后等待完成

# LFS 对象必须在本地磁盘（file_store=1）
sudo gitlab-rails runner 'puts LfsObject.where.not(file_store: 1).count'
# 若 > 0：配置好本地存储后 sudo gitlab-rake gitlab:lfs:migrate_to_local

# 子组数量（rgit 会确定性拍平路径并物化继承权限，仅用于迁移预估）
sudo gitlab-rails runner 'puts Group.where.not(parent_id: nil).count'

# 不要假定 omnibus 默认目录；核对 DB socket、repository storage 与 LFS 路径
sudo grep -E "db_host|git_data_dirs|repository_storages|lfs_storage_path" /etc/gitlab/gitlab.rb
sudo sed -n '/production:/,/^[^ ]/p' /var/opt/gitlab/gitlab-rails/etc/database.yml
```

## 1. 停写

```bash
sudo gitlab-ctl stop puma sidekiq   # 保留 postgresql 运行
```

## 2. 执行迁移（NAS 本机）

当前观测到的 NAS 配置使用外部 PostgreSQL socket `/var/run/postgresql`，默认
repository storage 为 `/zfs/gitlab_data/repositories`，LFS 为
`/zfs/gitlab_data/lfs-objects`。本机 peer map 允许 `git` 系统用户以数据库角色
`gitlab` 连接，同时该用户也能读取私有仓库/LFS；先把迁移结果写入独立目录：

```bash
sudo install -d -m 0700 -o git -g git /opt/usr/local/rgit/migration
sudo -u git /opt/usr/local/rgit/bin/rgit-migrate \
  --pg "postgres://gitlab@localhost/gitlabhq_production?host=/var/run/postgresql" \
  --gitlab-repos /zfs/gitlab_data/repositories \
  --gitlab-lfs   /zfs/gitlab_data/lfs-objects \
  --out          /opt/usr/local/rgit/migration/data \
  --dry-run > /opt/usr/local/rgit/migration/dry-run.json
```

若目标 NAS 配置不同，以 `database.yml` 和 `gitlab.rb` 的实际值为准。SQLx 的
socket URI 仍需提供非空 URL host（上例中的 `localhost`），真正的 socket 目录
由查询参数 `host=/var/run/postgresql` 指定。

NAS 生产切换使用共享存储模式：确认 dry-run JSON 的 `errors` 为空并停止 GitLab
写入组件后，去掉 `--dry-run`、增加 `--reuse-storage` 正式执行。该模式只生成
SQLite 与报告，不复制 `/zfs/gitlab_data`，不运行 repack、refs 对比或 `git fsck`：

```bash
sudo -u git /opt/usr/local/rgit/bin/rgit-migrate \
  --pg "postgres://gitlab@localhost/gitlabhq_production?host=/var/run/postgresql" \
  --gitlab-repos /zfs/gitlab_data/repositories \
  --gitlab-lfs /zfs/gitlab_data/lfs-objects \
  --out /opt/usr/local/rgit/migration/data \
  --reuse-storage
sudo chown -R git:git /opt/usr/local/rgit/migration/data
sudo rmdir /opt/usr/local/rgit/data       # 必须仍为空；失败就停止，不要强删
sudo mv /opt/usr/local/rgit/migration/data /opt/usr/local/rgit/data
```

`--out` 必须不存在或为空；迁移先在同一文件系统的临时目录完成，全部校验通过后
原子发布。失败时不会留下可启动的半成品数据目录。

若 GitLab 项目分布在多个 repository storage，必须为每个非默认存储重复传入：

```bash
--gitlab-storage fast=/mnt/fast/git-data/repositories \
--gitlab-storage archive=/mnt/archive/git-data/repositories
```

工具做的事（详见 DESIGN.md §13.3）：

1. 读 PG 必要表/列 → 写 `/opt/usr/local/rgit/data/rgit.db`（拒绝覆盖已存在的库）
2. `--reuse-storage`：只检查每个 hashed repo 与 LFS 文件存在且大小符合数据库，
   从 bare repo 的 `HEAD` 文本回填默认分支；不复制数据、不启动 Git 子进程
3. 未使用 `--reuse-storage` 时才拷贝仓库/LFS、归一化 alternates 并完整校验
4. 输出 `migration-report.json`；**errors 非空则退出码非零**

## 3. 迁移语义

| 内容 | 结果 |
|---|---|
| 用户（user_type=0） | 保留 id/用户名/邮箱/管理员标志；bcrypt 密码原样迁移，**原密码可直接登录**；非 active 状态一律转 blocked |
| 组/命名空间 | 保留 id；嵌套组拍平为 `父--子`，冲突追加 id；祖先组权限物化到后代组 |
| 项目 | 原 id 迁移（=disk_id，磁盘路径不变）；可见性/归档/描述/fork 关系保留 |
| 成员 | 项目成员、组成员（去掉未接受的 invite/request） |
| SSH key | 全文 + SHA256 指纹（bytea→base64 转换） |
| LFS | 对象与项目关联全量迁移 |
| PAT | **不迁移**（GitLab digest 掺实例盐不可移植）——用户重新签发 |
| 2FA | 不迁移，用户重新绑定 |
| issue/MR/CI/wiki 页面等 | 不迁移（非目标）；`.wiki.git`、`.design.git` 磁盘保留 |

## 4. 验收

```bash
# 起服务
sudo systemctl start rgit

# 用原 GitLab 密码登录 web，检查项目列表/成员/可见性
# 任选仓库通过 HTTPS/SSH clone、fetch、push，并验证 LFS 与 submodule；不要运行 fsck

# 数量对账（migration-report.json 与 GitLab 控制台数字一致）
cat /opt/usr/local/rgit/data/migration-report.json
```

确认无误后可 `gitlab-ctl stop` 停掉 GitLab（数据保留，随时回退）。
