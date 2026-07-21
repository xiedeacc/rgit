# rgit Project Rules

## Deployment And Testing

- The NAS (`ssh nas`, currently `192.168.2.247`) is the only machine on which
  the rgit service may be deployed or run.
- The dev machine is only a build machine and Git/API test client. Do not run
  rgit or a local rgit reverse proxy on dev.
- Keep all dev test worktrees, credentials, logs, and generated test data under
  `/root/src/rust/rgit_test`, never under the source tree.
- On dev, `rgit.xiedeacc.com` must resolve to the NAS and is the only HTTP(S)
  endpoint used for deployment and testing.
- The original GitLab deployment is permanently disabled. Never start
  `gitlab-runsvdir.service` or run `gitlab-ctl start`/`restart`; production
  operation and all protocol tests use rgit only.
- Deploy to `/opt/usr/local/rgit/{bin,conf,data,logs}` on the NAS and run all
  protocol smoke tests against that deployed instance.
- Production deployment must be performed only by running
  `/root/src/rust/rgit/scripts/deploy.sh` from the dev checkout. Do not run an
  ad hoc `ssh`/`scp`/`install` deployment sequence except when repairing that
  script itself. The script builds on dev, uploads artifacts and systemd units
  to the NAS, installs them under `/opt/usr/local/rgit`, restarts rgit, and
  verifies the deployed Web bundle.
- After every code, documentation, or configuration modification, commit all
  changed and untracked project files, push the current branch, and deploy to
  the NAS by running `/root/src/rust/rgit/scripts/deploy.sh` from the dev
  checkout.
- GitLab must remain stopped while rgit accesses the shared storage.

## Storage And Migration

- Repositories exist only at `/zfs/gitlab_data/repositories` and LFS objects
  only at `/zfs/gitlab_data/lfs-objects`; rgit shares both paths with GitLab.
- Never create `data/repositories` or `data/lfs-objects` under the rgit install
  or test data directory.
- Migration writes only SQLite metadata and its report. Do not copy
  `/zfs/gitlab_data`, repack repositories, or run `git fsck`.
- GitHub backup mirrors `/opt/usr/local/rgit/{bin,conf,data}` through
  `/opt/usr/local/rgit/.backup-worktree`. Never back up `/zfs/gitlab_data`,
  repositories, LFS object content, logs, or SSH credentials to GitHub.

## SSH

- Git SSH uses system OpenSSH on port `10022` and the forced-command
  `rgit-shell`; the project does not ship an embedded SSH server.
