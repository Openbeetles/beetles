# Skills directory

Copy or symlink skill markdown files from `storage_data/skills/` if needed (e.g. daily-briefing.md, weather.md).

Authoring source for the official office doctrine skill lives in:

- `skills/office-suite-doctrine/`

The deployed Beetle runtime artifact is:

- `storage_data/skills/office-suite-doctrine.md`

**安全约定 / Safety:** 命名禁止 `..`、`/`、`\`；单文件最大 32 KiB（`MAX_SKILL_CONTENT_LEN`），总数最多 64 个（`MAX_SKILL_COUNT`）。内容请勿包含高风险 shell 片段（如 `rm -rf /`、`curl | sh`），加载失败不阻塞启动。
