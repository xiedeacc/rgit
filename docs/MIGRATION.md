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

# 子组（rgit 不支持嵌套组，需先在 GitLab 里拍平）
sudo gitlab-rails runner 'puts Group.where.not(parent_id: nil).count'
```

## 1. 停写

```bash
sudo gitlab-ctl stop puma sidekiq   # 保留 postgresql 运行
```

## 2. 执行迁移（NAS 本机）

```bash
sudo -u gitlab-psql /opt/usr/local/rgit/bin/rgit-migrate run \
  --pg "postgres://gitlab-psql@/gitlabhq_production?host=/var/opt/gitlab/postgresql" \
  --gitlab-repos /var/opt/gitlab/git-data/repositories \
  --gitlab-lfs   /var/opt/gitlab/gitlab-rails/shared/lfs-objects \
  --out          /opt/usr/local/rgit/data
```

先加 `--dry-run` 演练一遍，确认报告无 error 再正式执行。

工具做的事（详见 DESIGN.md §13.3）：

1. 读 PG 必要表/列 → 写 `/opt/usr/local/rgit/data/rgit.db`（拒绝覆盖已存在的库）
2. 按 `sha256(project_id)` 逐项目拷贝 `@hashed` 仓库（含 `.wiki.git`）
3. 有 `objects/info/alternates` 的 fork 仓库执行 `repack -a -d` 归一化 + fsck
4. 按 oid 清单拷贝并校验 LFS 对象
5. 校验：每个项目 `git rev-parse` 可用、每个 LFS 对象大小一致，从磁盘回填 default_branch
6. 输出 `migration-report.json`；**errors 非空则退出码非零**

## 3. 迁移语义

| 内容 | 结果 |
|---|---|
| 用户（user_type=0） | 保留 id/用户名/邮箱/管理员标志；bcrypt 密码原样迁移，**原密码可直接登录**；非 active 状态一律转 blocked |
| 组/命名空间 | 根级 User/Group namespace 原 id 迁移；子组必须先拍平 |
| 项目 | 原 id 迁移（=disk_id，磁盘路径不变）；可见性/归档/描述/fork 关系保留 |
| 成员 | 项目成员、组成员（去掉未接受的 invite/request） |
| SSH key | 全文 + SHA256 指纹（bytea→base64 转换） |
| LFS | 对象与项目关联全量迁移 |
| PAT | **不迁移**（GitLab digest 掺实例盐不可移植）——用户重新签发 |
| 2FA | 不迁移，用户重新绑定 |
| issue/MR/CI/wiki 页面等 | 不迁移（非目标）；`.wiki.git` 磁盘保留 |

## 4. 验收

```bash
# 起服务
sudo systemctl start rgit

# 用原 GitLab 密码登录 web，检查项目列表/成员/可见性
# 任选仓库 clone 一次全量对比：
git clone https://git.example.com/ns/proj.git /tmp/check && git -C /tmp/check fsck

# 数量对账（migration-report.json 与 GitLab 控制台数字一致）
cat /opt/usr/local/rgit/data/migration-report.json
```

确认无误后可 `gitlab-ctl stop` 停掉 GitLab（数据保留，随时回退）。
