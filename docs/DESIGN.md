# rgit 系统设计

rgit 是一个面向小团队的轻量级自托管 Git 服务：Rust 后端 + Flutter Web 前端，
SQLite 单文件数据库，磁盘存储格式与 GitLab 兼容（hashed storage / LFS 布局），
可将现有 GitLab（omnibus, v17.11.x）实例的数据无损迁移过来。

- 后端：Rust（axum + sqlx/SQLite），`rgit` + system OpenSSH forced command
- 前端：Flutter Web（GitHub 风格 UI），构建产物为静态文件由 `rgit` 直接托管
- 部署：NAS `/opt/usr/local/rgit/{bin, conf, data, logs}`，nginx 反向代理 + TLS
- 备份：参照 rblog 的 bash + systemd timer，镜像 `bin/conf/data` 到
  `git@github.com:xiedeacc/rgit_data.git`，不备份共享仓库与 LFS

## 1. 目标与非目标

### 1.1 功能集（目标）

| 功能 | 说明 |
|---|---|
| 用户/登录/鉴权 | 密码登录（bcrypt）、cookie session、管理员面板、封禁用户 |
| Personal Access Token | 带 scope 的 PAT，用于 API 与 git HTTP 认证，可代码访问 |
| SSH 公钥管理 | 用户自助上传公钥，SSH 协议按指纹认证 |
| Git 协议 | HTTPS + SSH 完整支持 clone / fetch / push / submodule / shallow / archive |
| Git LFS | 标准 LFS batch API，存储布局与 GitLab 兼容 |
| 项目管理 | 创建、删除、改名、转移、归档（archive）、fork、可见性(私有/内部/公开) |
| 组（group） | 一层扁平 group 作为命名空间 + 组成员，满足小团队共享 |
| 代码浏览 | 文件树、blob、raw、提交历史、分支/标签、README 渲染、tar.gz/zip 下载 |
| 管理面板 | 用户增删改/封禁、项目总览、系统状态 |
| 备份 | rblog 式：`bin/conf/data` 镜像到 GitHub，明确排除仓库与 LFS |
| 迁移 | `rgit-migrate` 工具把 GitLab PG 中必要表/列迁到 SQLite，仓库与 LFS 原地复用 |

### 1.2 非目标（明确不做）

CI/CD、Issue、MR/PR、Milestone、Todo、Activity/Feed、邮件通知、Prometheus、
Redis、PostgreSQL、Import（除 GitLab 迁移外）、Wiki 页面（磁盘上 `.wiki.git`
迁移时保留不丢，但不提供 UI）、Container Registry、Pages、Webhook、
GitLab 协议/API 兼容（协议自定，仅磁盘格式兼容）。

## 2. 总体架构

```
                    ┌────────────────────────── NAS ──────────────────────────┐
                    │                                                          │
  git clone https:// ─┐   ┌─────────┐  proxy_pass   ┌───────────────────────┐ │
  浏览器 (Flutter Web) ├──▶│  nginx  │──────────────▶│  rgit (单二进制)        │ │
  API (PAT)          ─┘   │ :443 TLS│  127.0.0.1:8000│  ├─ axum HTTP server  │ │
                    │     └─────────┘                │  │   ├─ /api/v1  REST │ │
  git clone ssh:// ───────────────▶ system sshd ────▶│  │   ├─ git smart HTTP│ │
                    │                                │  │   ├─ git LFS       │ │
                    │                                │  │   └─ Flutter 静态页 │ │
                    │                                │  └─ rgit-shell        │ │
                    │                                └──────────┬────────────┘ │
                    │                                           │ spawn        │
                    │        ┌──────────────────────────────────▼────────────┐ │
                    │        │ git upload-pack / receive-pack / archive ...  │ │
                    │        └──────────────────────────────────┬────────────┘ │
                    │   ┌───────────────┐   ┌───────────────────▼────────────┐ │
                    │   │ data/rgit.db  │   │ /zfs/gitlab_data/repositories │ │
                    │   │ (SQLite WAL)  │   │ /zfs/gitlab_data/lfs-objects  │ │
                    │   └───────────────┘   └────────────────────────────────┘ │
                    │        ▲ systemd timer 每小时                             │
                    │   ┌────┴────────────┐        push                        │
                    │   │ rgit-backup.sh  │──────────────▶ github:rgit_data    │
                    │   └─────────────────┘                                    │
                    └──────────────────────────────────────────────────────────┘
```

要点：

- 生产 SSH 复用 system OpenSSH。`authorized_keys` forced command 调用
  `rgit-shell`；没有 redis、没有队列。git 操作全部 spawn 系统 `git`
  （同 gitlab-shell/gitolite 思路，不用 libgit2）。
- **SQLite WAL 模式**，单写多读，小团队规模绰绰有余；所有整型常量
  （visibility、access_level）与 GitLab 相同，迁移即拷贝。
- **nginx 只反代 HTTP**（TLS 终止、大包体透传）；system sshd 继续监听 10022。

### 2.1 crate 划分

```
crates/
├── rgit          二进制入口：加载配置、初始化并启动 HTTP
├── rgit-core     领域层：config / db(migrations) / models / auth / perm / storage
├── rgit-git      git 进程封装：仓库管理(init/fork/delete/archive)、协议(stateless-rpc)、
│                 读取(ls-tree/cat-file/log/refs)
├── rgit-http     axum：REST API、git smart HTTP、LFS、静态资源、中间件
├── rgit-ssh      system OpenSSH forced-command / rgit-shell 命令校验
└── rgit-migrate  迁移二进制：GitLab PG → SQLite + 磁盘存储归一化
web/              Flutter Web 前端
conf/             rgit.example.toml、nginx、systemd 样例
scripts/          rgit-backup.sh、deploy.sh
docs/             DESIGN.md（本文）、MIGRATION.md、DEPLOY.md
```

## 3. 部署布局（NAS）

```
/opt/usr/local/rgit/
├── bin/
│   ├── rgit                  # 服务二进制
│   ├── rgit-migrate          # 迁移工具（迁移完成后可删）
│   ├── rgit-backup           # 备份脚本（scripts/rgit-backup.sh）
│   └── web/                  # Flutter build 产物（flutter build web 的 build/web）
├── conf/
│   └── rgit.toml             # 0600，运行配置
├── data/                     # ← GitHub 备份对象
│   ├── rgit.db               # SQLite（WAL: rgit.db-wal / rgit.db-shm）
│   └── migration-report.json
├── logs/                     # 不参与备份
└── .backup-worktree/         # 备份镜像工作区（git repo，不参与备份）
```

仓库固定使用 `/zfs/gitlab_data/repositories`，LFS 固定使用
`/zfs/gitlab_data/lfs-objects`，与 GitLab 共享且不在 rgit `data/` 下。

systemd 单元（模板见 `scripts/deploy.sh`）：

- `rgit.service` — 主服务，`RGIT_CONFIG=/opt/usr/local/rgit/conf/rgit.toml`
- `rgit-backup.service` — oneshot，执行 `bin/rgit-backup`
- `rgit-backup.timer` — `OnBootSec=5min` / `OnUnitActiveSec=1h` / `Persistent=true`

## 4. 配置设计（TOML）

层级：内置默认 → `./rgit.toml` → `$RGIT_CONFIG` 指定文件 → 环境变量
`RGIT__SECTION__KEY`（与 rblog 相同的分层约定）。完整注释版见
`conf/rgit.example.toml`；结构体定义 `crates/rgit-core/src/config.rs`。

```toml
[http]
bind = "127.0.0.1:8000"                  # nginx 反代到这里
external_url = "https://git.example.com" # 拼 clone URL / LFS href 用

[ssh]
enabled = true
clone_host = "git.example.com"           # 展示用 ssh clone 地址
clone_port = 10022
authorized_keys_file = "/opt/usr/local/rgit/data/ssh/authorized_keys"
shell_path = "/opt/usr/local/rgit/bin/rgit-shell"
shell_config = "/opt/usr/local/rgit/conf/rgit.toml"

[db]
path = "/opt/usr/local/rgit/data/rgit.db"

[storage]
repositories = "/zfs/gitlab_data/repositories"
lfs_objects  = "/zfs/gitlab_data/lfs-objects"

[git]
bin = "git"
timeout_secs = 3600                      # 单个 git 子进程硬超时
max_concurrent_operations = 16           # HTTP/SSH/浏览/archive 全局 Git 并发上限
queue_timeout_secs = 30                  # 等待并发槽位超时后返回 503

[web]
static_dir = "/opt/usr/local/rgit/bin/web"

[auth]
session_ttl_hours = 336                  # 14 天
bcrypt_cost = 12
min_password_length = 10
max_login_failures = 10                  # 连续失败即临时锁定
lockout_minutes = 15

[lfs]
enabled = true
max_file_size = 0                        # 0 = 不限

[log]
level = "info"                           # tracing filter
```

## 5. 数据库设计（SQLite）

原则：**不兼容 GitLab 库表**（只迁必要数据），但**整型常量兼容**
（visibility 0/10/20；access_level 10/20/30/40/50），使迁移零转换。
时间戳一律 TEXT ISO-8601 UTC；布尔 INTEGER 0/1。
DDL 全文见 `crates/rgit-core/migrations/0001_init.sql`，要点：

| 表 | 关键列 | 说明 |
|---|---|---|
| users | username/email UNIQUE NOCASE, password_hash, is_admin, state | password_hash 直接存 GitLab 的 bcrypt `encrypted_password`，**迁移用户密码不变** |
| namespaces | path UNIQUE, kind('user'\|'group'), owner_user_id | URL 第一段；每个用户一个 user namespace；group 扁平（不做子组） |
| group_members | (namespace_id,user_id) PK, access_level | |
| projects | (namespace_id,path) UNIQUE, visibility, archived, default_branch, **disk_id UNIQUE, disk_hash UNIQUE**, forked_from_project_id, lfs_enabled | `disk_id` 决定磁盘路径且**不可变**；迁移项目 = GitLab project id，新项目 = 自增 id。`disk_hash = hex(sha256(disk_id 十进制串))` 落库缓存 |
| project_members | (project_id,user_id) PK, access_level | |
| ssh_keys | key, fingerprint_sha256 UNIQUE(无填充 base64), last_used_at | GitLab `keys.fingerprint_sha256` 是 bytea 原始 32 字节，迁移时转无填充 base64 |
| personal_access_tokens | token_hash UNIQUE = hex(sha256(token)), scopes(JSON), expires_at, revoked | GitLab 的 token_digest 掺了实例盐（db_key_base 前 32 字节），**不可迁移，用户需重新签发** |
| sessions | id PK = hex(sha256(cookie 值)), expires_at | 库泄露不可回放 |
| lfs_objects | oid UNIQUE, size | 全局内容寻址，同 GitLab 去重模型 |
| project_lfs_objects | (project_id, lfs_object_id) PK | 决定某项目能否下载某对象 |

权限解析不做 GitLab 的 project_authorizations 缓存表——小团队规模直接
`max(项目成员, 组成员, namespace owner=50)` 一条 SQL 现算（`rgit-core/src/auth/mod.rs`）。

## 6. 磁盘存储布局（与 GitLab 逐字节兼容）

来源：gitlab-foss `app/models/storage/hashed.rb`、`app/uploaders/lfs_object_uploader.rb`。

| 对象 | 路径公式 |
|---|---|
| 项目仓库 | `<repos>/@hashed/H[0:2]/H[2:4]/H.git`，`H = hex(SHA256(disk_id 的十进制 ASCII))` |
| wiki 仓库 | 同上 `H.wiki.git`（迁移保留，不提供功能） |
| LFS 对象 | `<lfs>/oid[0:2]/oid[2:4]/oid[4:]`，oid = 内容 SHA256 十六进制 |

实现：`rgit-core/src/storage.rs`（含与 GitLab 已知样例的单元测试：
sha256("1") = `6b86b2...`）。

**fork 的实现**：`git clone --bare --local`（同盘硬链接对象，磁盘开销近零），
不引入 GitLab 的 `@pools` 对象池与 alternates 机制——迁移时会把带 alternates
的仓库 `repack -a -d` 归一化成自包含仓库（见 §13.4），归一化后 `@pools` 不再需要。

**archive（归档）**：`projects.archived = 1`。归档项目：拒绝一切写
（HTTP/SSH push、LFS 上传、设置修改除 unarchive 外），读/clone 正常，UI 打标。

**删除项目**：DB 删行（外键级联清 members/lfs 关联），仓库目录改名为
`<path>.deleted.<timestamp>` 延迟 rm（防误删，可配置立刻删）；LFS 对象引用清零后
由每日清理任务物理删除。

## 7. 认证与鉴权

### 7.1 身份凭证（三种）

| 凭证 | 用于 | 存储 |
|---|---|---|
| 密码 | Web 登录、git HTTP Basic | bcrypt（新建 cost 12；GitLab 迁来的 cost 13 验证兼容，因为 cost 编码在 hash 里） |
| PAT `rgit_<40位base62>` | REST API（`Authorization: Bearer` 或 `PRIVATE-TOKEN`）、git HTTP Basic 的密码位 | 仅存 hex(sha256(token))，创建时明文只显示一次 |
| SSH 公钥 | git SSH | 全文 + SHA256 指纹（无填充 base64） |

PAT scope（枚举 `rgit-core/src/auth/token.rs`）：

- `api`：完整读写 API + 仓库读写
- `read_api`：只读 API
- `read_repository`：git HTTP 拉取 + LFS 下载
- `write_repository`：git HTTP 推送 + LFS 上传（蕴含 read_repository）

### 7.2 会话

- Cookie `rgit_session`：32 字节随机 → base64url；`HttpOnly; Secure; SameSite=Lax; Path=/`
- 服务端存 sha256；TTL 14 天（可配）；登录成功即新建（防会话固定），登出即删；
  后台任务定期清理过期行。
- CSRF：SameSite=Lax + **API 全部为 JSON（要求 `Content-Type: application/json`）
  且变更类请求校验自定义头 `X-Rgit-Csrf: 1`**（跨站表单无法携带自定义头），
  双保险，无需 token 轮转。git/LFS 端点不认 cookie，只认 Basic/PAT，无 CSRF 面。

### 7.3 权限模型

访问级别（与 GitLab 同值）：Guest 10 / Reporter 20 / Developer 30 /
Maintainer 40 / Owner 50。项目动作要求：

| 动作 | 匿名 | 要求 |
|---|---|---|
| 读（浏览/clone/fetch/archive 下载/LFS 下载） | 仅 public 项目 | 登录用户：public/internal 直接可读；private 需 ≥Guest |
| 写（push / LFS 上传） | 拒绝 | ≥Developer，且项目未归档 |
| 管理（设置/成员/删除/归档/转移） | 拒绝 | ≥Maintainer（删除/转移要求 Owner 或 admin） |
| 建项目 | — | 任何活跃用户（自己 namespace）；group 内需 ≥Maintainer |
| 管理面板 | — | is_admin |

实现单点：`rgit_core::auth::authorize_repo(db, user, project, action)`，
HTTP/SSH/LFS 三个入口共用，杜绝旁路。admin 全通过；被封禁（state != 'active'）
一律拒绝（含已有 session/PAT/SSH key）。

### 7.4 登录防爆破

内存表（用户名+IP）计失败次数，超过 `max_login_failures` 锁 `lockout_minutes`；
恒定时序：用户不存在时也跑一次 bcrypt（dummy hash）再拒绝。

### 7.5 安全清单

- 全部 SQL 走 sqlx 参数绑定；无字符串拼接 SQL
- 路径安全：namespace/project path 白名单正则 `^[a-zA-Z0-9][a-zA-Z0-9_.-]*$`
  （另拒绝 `.git`/`.wiki` 后缀、`.`/`..`、`@` 开头），磁盘路径只由 disk_hash 推导，
  用户输入永不进入文件路径
- git 子进程一律 `Command::args`（无 shell），工作目录/仓库路径用绝对路径，
  设置 `GIT_PROTOCOL=version=2`，超时强杀；receive-pack/upload-pack 的 stdin/stdout
  全程流式，不落内存
- 上传（LFS/push）流式写盘 + 临时文件 + fsync + 原子 rename；LFS 上传完成后
  校验 sha256 与声明 oid 一致，不一致即丢弃
- 秘密文件权限：rgit.toml 0600、data/ssh 0700、rgit.db 0600
- nginx 加安全头（HSTS、X-Content-Type-Options、frame-deny）；raw blob 响应
  `Content-Type` 白名单 + `X-Content-Type-Options: nosniff`，HTML 一律按
  text/plain 输出防存储型 XSS
- 时序安全比较：token/会话查找按 sha256 摘要索引（预映像安全），密码走 bcrypt 内建

## 8. Git 协议实现

### 8.1 smart HTTP（读写全支持，git ≥ 2.53，protocol v2）

路由（挂在 `/{namespace}/{project}.git/` 下，`rgit-http/src/handlers/git_http.rs`）：

```
GET  /{ns}/{proj}.git/info/refs?service=git-upload-pack    # 能力广告（clone/fetch）
GET  /{ns}/{proj}.git/info/refs?service=git-receive-pack   # 能力广告（push）
POST /{ns}/{proj}.git/git-upload-pack                      # fetch/clone 数据
POST /{ns}/{proj}.git/git-receive-pack                     # push 数据
```

实现方式：**直接 spawn `git upload-pack --stateless-rpc [--advertise-refs]`
与 `git receive-pack --stateless-rpc [--advertise-refs]`**（不经 git http-backend，
省去 CGI 环境模拟；行为与 GitLab workhorse 相同）：

1. info/refs：响应头 `Content-Type: application/x-{service}-advertisement`、
   `Cache-Control: no-cache`；正文先写 pkt-line `# service={service}\n` + flush-pkt
   `0000`，再接子进程（`--advertise-refs`）stdout。
2. POST：请求体（`Content-Encoding: gzip` 时先解压）流式接到子进程 stdin，
   stdout 流式回客户端（`Content-Type: application/x-{service}-result`）。
   HTTP/1.1 chunked / HTTP2 由 axum/hyper 处理。
3. 认证：`Authorization: Basic user:password` 或 `user:PAT`。upload-pack 对
   public 项目允许匿名；私有项目 / receive-pack 未认证返回 401 +
   `WWW-Authenticate: Basic realm="rgit"`，git 客户端会提示输入凭证。
   PAT 走 scope 校验（read_repository / write_repository）。
4. push 后钩子逻辑（进程内，不用 hook 脚本）：receive-pack 正常退出后
   更新 `projects.default_branch`（若 HEAD 变化）、`updated_at`。
   protected branch 简化为：非 Maintainer 禁止删除/强推 default_branch
   （通过 pre-receive 环境注入实现，v1 可先不做，接口留好）。

submodule 无需服务端特殊支持（客户端多次 clone）；shallow/partial clone
（`--depth`、`--filter=blob:none`）由 upload-pack 原生支持。

### 8.2 SSH（系统 sshd + rgit-shell）

生产使用系统 OpenSSH 监听 `10022`。`rgit` 从 SQLite 原子生成 authorized_keys，
每个 key id 绑定 forced command；OpenSSH 完成公钥认证后以 `git` 用户执行
`rgit-shell`。shell 不解释客户端命令，只解析严格白名单：

- `git-upload-pack`、`git-receive-pack`、`git-upload-archive`
- `git-lfs-authenticate <path> <upload|download>`，返回一小时有效、单项目且区分
  读写的 LFS Basic credential；该 credential 不能调用 API 或 Git pack HTTP
- path → 项目 → `authorize_repo`，归档项目拒绝 receive-pack/LFS upload
- authorized_keys 禁止 shell/PTY、agent/X11/端口转发；SSH key 增删后原子重建

rgit 不包含内嵌 SSH server；SSH 始终由 system OpenSSH 接入，再通过
`authorized_keys` forced-command 调用 `rgit-shell`。

- clone 地址展示：`ssh://git@{clone_host}:{clone_port}/{ns}/{proj}.git`
  （非 22 端口必须用 ssh:// 形式）

### 8.3 归档下载

`GET /{ns}/{proj}/-/archive/{ref}/{proj}-{ref}.{tar.gz|zip}`（同时暴露
`/api/v1/projects/.../archive`）。实现：`git archive --format=... <ref>`
stdout 流式回传。ref 白名单校验（存在的 branch/tag/sha，防参数注入）。

## 9. Git LFS

客户端是标准 git-lfs，因此 **batch API 必须遵循 LFS 规范**（这不属于
"gitlab 协议兼容"，是 LFS 开放标准）；传输端点自定义：

```
POST /{ns}/{proj}.git/info/lfs/objects/batch    # application/vnd.git-lfs+json
GET  /{ns}/{proj}.git/info/lfs/objects/{oid}    # 下载（basic transfer）
PUT  /{ns}/{proj}.git/info/lfs/objects/{oid}    # 上传（body = 原始字节流）
POST /{ns}/{proj}.git/info/lfs/verify           # 上传后校验（oid+size）
```

- batch：operation=download → 校验 Read 权限 + `project_lfs_objects` 有链接
  → 返回带一次性 href 的 actions（直接用同一 Basic/PAT 凭证，无独立临时 token，
  href 即上面 GET）；operation=upload → 校验 Write → 对已存在 oid（全局去重）
  只补 project 链接、不返回 upload action。
- 上传：流式写 `lfs-objects/tmp/<uuid>`，边写边算 sha256，完成后与 oid、size
  比对，通过则原子 rename 到 `oid[0:2]/oid[2:4]/oid[4:]` 并插入
  `lfs_objects` + `project_lfs_objects`。
- 下载：`tokio_util::io::ReaderStream` 直接回文件，`Content-Length` 为 size。
- fork 时把父项目全部 `project_lfs_objects` 链接复制给子项目（对象文件共享）。

## 10. REST API 设计（/api/v1）

约定：JSON；认证 `Authorization: Bearer <PAT>` / `PRIVATE-TOKEN: <PAT>` /
session cookie；错误统一 `{"error": "...", "message": "..."}`；
分页 `?page=1&per_page=20`，响应头 `X-Total`。项目可用 `{id}` 或
URL-encode 的 `{ns%2Fpath}` 定位（同 GitLab 习惯）。

```
# 会话
POST   /api/v1/session                      # {login, password} → set-cookie
DELETE /api/v1/session
GET    /api/v1/user                          # 当前用户
PATCH  /api/v1/user                          # 改 name/email
POST   /api/v1/user/password                 # 改密码（需旧密码）

# SSH keys / PAT（自助）
GET/POST       /api/v1/user/keys             # POST {title, key}
DELETE         /api/v1/user/keys/{id}
GET/POST       /api/v1/user/tokens           # POST {name, scopes, expires_at} → 明文一次
DELETE         /api/v1/user/tokens/{id}      # 撤销

# 项目
GET    /api/v1/projects                      # 可见项目列表 ?search=&visibility=&page=
POST   /api/v1/projects                      # {name, path, namespace_id?, visibility, description}
GET    /api/v1/projects/{id|ns%2Fpath}
PATCH  /api/v1/projects/{id}                 # 名称/描述/可见性/default_branch/lfs_enabled
DELETE /api/v1/projects/{id}                 # Owner/admin
POST   /api/v1/projects/{id}/archive
POST   /api/v1/projects/{id}/unarchive
POST   /api/v1/projects/{id}/fork            # {namespace_id?, path?, name?}
POST   /api/v1/projects/{id}/transfer        # {namespace_id}

# 项目成员
GET/POST/PATCH/DELETE /api/v1/projects/{id}/members[/{user_id}]

# 仓库浏览（全部 Read 权限）
GET /api/v1/projects/{id}/repository/tree?ref=&path=&page=
GET /api/v1/projects/{id}/repository/blob?ref=&path=        # 元数据+base64 内容(限 1MB)
GET /api/v1/projects/{id}/repository/raw?ref=&path=         # 原始字节流
GET /api/v1/projects/{id}/repository/commits?ref=&path=&page=
GET /api/v1/projects/{id}/repository/commits/{sha}          # 详情+diffstat
GET /api/v1/projects/{id}/repository/diff/{sha}             # patch
GET /api/v1/projects/{id}/repository/branches|tags
GET /api/v1/projects/{id}/repository/archive?ref=&format=
GET /api/v1/projects/{id}/repository/readme?ref=            # 定位+渲染前的 raw markdown

# 组
GET/POST /api/v1/groups ; GET/PATCH/DELETE /api/v1/groups/{id}
GET/POST/PATCH/DELETE /api/v1/groups/{id}/members[/{user_id}]

# 管理面板（is_admin）
GET/POST  /api/v1/admin/users                # 建号（生成初始密码）
PATCH     /api/v1/admin/users/{id}           # 改角色/重置密码/block/unblock
DELETE    /api/v1/admin/users/{id}
GET       /api/v1/admin/projects             # 全量项目
GET       /api/v1/admin/stats                # 用户数/项目数/磁盘占用/版本
GET       /api/v1/admin/sessions             # 活跃会话，DELETE 强制下线
```

仓库读取实现（`rgit-git/src/read.rs`）：spawn git 明文命令并解析——
`ls-tree -z --long`、`cat-file --batch`、`log --format=%H%x00...%x00 -z`、
`for-each-ref`、`diff-tree`；全部只读、超时保护、输出上限保护。

## 11. Flutter Web 前端

- SDK：`/root/src/software/flutter`（3.44 / Dart 3.12）；`web/` 目录，
  `flutter build web` 产物拷到 `bin/web` 由 rgit 托管（SPA fallback 到 index.html）
- 依赖：`go_router`（URL 路由，路径与后端项目路径一致）、`provider`（状态）、
  `http`（API client）、`flutter_markdown`（README）、`google_fonts` 可选
- UI 风格：GitHub 式——顶栏（logo/搜索/头像菜单）、仓库页左文件树 tab 布局、
  等宽字体代码视图、浅/深色主题

页面（`web/lib/pages/`）：

| 路由 | 页面 |
|---|---|
| /login | 登录 |
| / | Explore：可见项目列表 + 搜索 |
| /:ns | 用户/组主页（项目列表） |
| /:ns/:proj | 仓库首页：文件树 + README + clone 地址（https/ssh）+ 分支切换 |
| /:ns/:proj/tree/:ref/* , /blob/:ref/* | 树/文件浏览（代码高亮 highlight 包） |
| /:ns/:proj/commits/:ref , /commit/:sha | 提交列表 / 详情 diff |
| /:ns/:proj/branches , /tags | 分支/标签（含 archive 下载） |
| /:ns/:proj/settings | 项目设置：常规/成员/危险区（归档/转移/删除） |
| /settings/profile , /keys , /tokens | 个人设置 |
| /admin , /admin/users , /admin/projects | 管理面板 |
| /groups/new , /:ns/settings | 组管理 |

API client（`web/lib/api/client.dart`）：统一封装 fetch + JSON + 错误 +
cookie（浏览器自动带）+ `X-Rgit-Csrf: 1` 头；401 全局跳登录。

## 12. 备份（参照 rblog）

实现 `scripts/rgit-backup.sh`（部署为 `bin/rgit-backup`），与 rblog 的
`rblog-backup.sh` 同构，保存 `bin/`、`conf/` 与 `data/`：

1. `ensure_repo`：`.backup-worktree` 不存在则 clone
   `git@github.com:xiedeacc/rgit_data.git`（失败退化 `git init` + remote add），
   存在则 fetch/checkout/pull --ff-only（容错 `|| true`）
2. `snapshot_sqlite`：对在线 SQLite 执行 `VACUUM INTO`，在 worktree 中生成
   一致的 `data/rgit.db`，不复制在线 WAL 数据库
3. `reset_generated_split_files`：还原上轮分块状态
4. `sync_source`：先清理目标目录，再复制 `bin/conf/data`；排除热数据库
   文件、锁文件、`repositories/` 和 `lfs-objects/`。`logs/`、`.ssh/` 与其他
   顶层目录不进入 worktree，运行用户的 GitHub 私钥绝不入库
5. `verify_mirror`：只校验 SQLite 快照完整性与必要 schema，不运行 Git 命令
6. `split_large_files`：>50MiB 文件分块 `.0/.1/...` + `.rgit-split` 标记 +
   gitignore 原文件
7. `commit_and_push_if_changed`：`git add -A`；有变更则
   `Backup 2026-07-15T12:00:00Z` 提交并 push；无变更但上次 push 失败会补推

认证：运行用户的 SSH key（部署时把 NAS 的 deploy key 加到 rgit_data 仓库）。
调度：`rgit-backup.timer` 每小时（`OnUnitActiveSec=1h`, `Persistent=true`)。
配置：环境变量 `RGIT_BACKUP_REPO_URL / RGIT_BACKUP_BRANCH / RGIT_BACKUP_ROOT /
RGIT_BACKUP_WORK_DIR / RGIT_BACKUP_MAX_FILE_BYTES / RGIT_BACKUP_SPLIT_BYTES`
（systemd unit 里 Environment= 注入，同 rblog）。

恢复：clone rgit_data → 按 `.rgit-split` 标记 `cat file.0 file.1 > file` 重组 →
拷回 `/opt/usr/local/rgit/{bin,conf,data}` → 校验 SQLite。仓库与 LFS 由共享
存储自身恢复，不进入此备份。
`scripts/rgit-restore.sh` 自动化以上步骤。

## 13. GitLab → rgit 迁移（rgit-migrate）

### 13.1 前提

- NAS 上 omnibus GitLab（17.11.x），数据在
  `/var/opt/gitlab/git-data/repositories`（hashed storage）与
  `/var/opt/gitlab/gitlab-rails/shared/lfs-objects`
- 迁移期间 GitLab 停写：`gitlab-ctl stop puma sidekiq`（保留 postgresql 运行）

### 13.2 数据源

PostgreSQL 直连：`rgit-migrate --pg postgres://gitlab-psql@/gitlabhq_production?host=/var/opt/gitlab/postgresql`
（unix socket，NAS 本机跑）。只读以下表/列：

| GitLab 表 | 取列 | → rgit 表 |
|---|---|---|
| users（user_type=0，排除 bot） | id, username, email, name, encrypted_password, admin, state | users（state: active→active，其余→blocked；encrypted_password 原样→password_hash） |
| namespaces（type='User'/'Group'） | id, name, path, type, owner_id, parent_id | namespaces（子组路径确定性拍平；继承成员权限物化） |
| projects | id, name, path, description, namespace_id, visibility_level, archived, lfs_enabled, repository_storage, storage_version | projects（disk_id=id, disk_hash=sha256(id)；storage_version<2 的 legacy 项目报错，需先在 GitLab 里迁 hashed） |
| members（type 区分，去 invite/request：user_id 非空且 requested_at 空） | source_type, source_id, user_id, access_level | project_members / group_members |
| keys（type='Key'） | user_id, title, key, fingerprint_sha256(bytea→无填充base64), last_used_at | ssh_keys |
| lfs_objects（file_store=1） | oid, size | lfs_objects（file_store=2 对象存储需先 `gitlab-rake gitlab:lfs:migrate_to_local`） |
| lfs_objects_projects | lfs_object_id, project_id | project_lfs_objects（去重） |
| fork_network_members | project_id, forked_from_project_id | projects.forked_from_project_id |
| projects.default_branch | 磁盘读：`git symbolic-ref HEAD` | projects.default_branch |

不迁：PAT（盐不可移植，重发）、2FA（用户重绑）、issue/MR/CI 等全部（非目标）。

### 13.3 步骤（`rgit-migrate`）

```
rgit-migrate \
  --pg "postgres://gitlab-psql@/gitlabhq_production?host=/var/opt/gitlab/postgresql" \
  --gitlab-repos /var/opt/gitlab/git-data/repositories \
  --gitlab-lfs   /var/opt/gitlab/gitlab-rails/shared/lfs-objects \
  --out          /opt/usr/local/rgit/data
```

1. 读 PG → 内存映射 → 在同盘 staging 目录写 `rgit.db`（单事务），成功后原子发布 `<out>`
2. 仓库拷贝：对每个项目按 `sha256(id)` 定位源目录，
   `cp -a` 到 `<out>/repositories/@hashed/...`（含 `.wiki.git`、`.design.git`）；
   源缺目录 → 记 ERROR（**清单必须为空才算成功，保证不丢项目**）
3. alternates 归一化：若 `objects/info/alternates` 存在（fork 池），
   在**目标副本**上 `git repack -a -d` + 删 alternates + `git fsck --connectivity-only`
4. LFS 拷贝：按 db oid 清单逐个拷贝并校验 size + SHA-256；缺文件 → ERROR
5. 校验报告：项目数/用户数/key 数/LFS 对象数与字节数逐项对账，源/目标 refs
   一致，所有主仓库和辅助仓库执行 `git fsck --full`；输出 `migration-report.json`

### 13.4 迁移后

管理员首次登录（GitLab 原密码），检查项目/成员/可见性；用户重发 PAT；
`--dry-run` 模式支持只读演练。原 GitLab 数据不动，可回退。

## 14. nginx 反向代理

`conf/nginx/rgit.conf`（要点）：

```nginx
server {
    listen 443 ssl http2;
    server_name git.example.com;
    # ssl_certificate ...;

    client_max_body_size 0;           # push/LFS 大包体
    proxy_request_buffering off;      # 流式上行（push/LFS 必需）
    proxy_buffering off;              # 流式下行（clone 大仓库）
    proxy_read_timeout 3600s;
    proxy_send_timeout 3600s;

    add_header Strict-Transport-Security "max-age=63072000" always;
    add_header X-Content-Type-Options nosniff always;
    add_header X-Frame-Options DENY always;

    location / {
        proxy_pass http://127.0.0.1:8000;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_http_version 1.1;
    }
}
```

SSH 不经 nginx，由 system sshd 直接监听生产端口 10022。

## 15. 里程碑

| 阶段 | 内容 | 验收 |
|---|---|---|
| M1 | 设计 + 骨架（本次） | cargo check / flutter analyze 通过 |
| M2 | auth + 用户/项目/成员 CRUD + git smart HTTP | curl API 全通；https clone/push 真仓库往返 |
| M3 | SSH + LFS + archive + fork | ssh clone/push；git lfs push/pull；fork 后独立推拉 |
| M4 | Flutter UI 全页面 | 浏览器完成登录→建项目→浏览→设置全流程 |
| M5 | rgit-migrate + 备份 + NAS 部署 | 生产 GitLab 演练迁移对账零丢失；定时备份可恢复 |
