# 🛡️ Kyvault 密钥与资产台账管理规则 (Global Agent Rule)

任何参与本项目和本地开发的 AI Agent (Claude, Codex, Gemini 等) 必须严格遵守以下关于密钥与台账管理的规则：

1. **密钥零硬编码原则**：绝对禁止在任何代码、配置文件、Git 提交记录、AI 回复中泄露明文密钥、服务器密码或私有 Token。所有敏感数据必须加密存取。
2. **使用 cs secrets**：查询键名使用 `cs secrets list`，读取数据使用 `cs secrets get secret://<path>`，注入子进程使用 `cs secrets run --env KEY=secret://path -- <cmd>`。
3. **时效更新与覆写**：针对 Agent 容易陷入的“只做加法、即便密钥已失效也保留旧值”的错误倾向，做出刚性规定：一旦检测到密钥库中的某个 Key 失效（接口报错 401、Token 过期或校验失败），Agent 必须主动改写/覆写（Overwrite）该密钥，绝对不能予以保留或盲目重试。对于失效的旧密钥，必须通过 `cs secrets set` 写入新密钥，或引导用户进行轮换。
4. **多账号自适应路由**：针对飞书 (Feishu)、GitLab、GitHub 等多账号/多 Profile 场景，Agent 应当采用 `secret://cli/<cli_name>/<profile>` 进行细分寻址，并使用 `cs secrets run --env TOKEN=secret://...` 在运行时单向注入，避免污染本地全局 CLI 配置。
