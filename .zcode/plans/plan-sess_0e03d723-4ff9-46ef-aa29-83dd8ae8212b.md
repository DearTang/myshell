# AI 多模型支持 + 预设配置

## 架构设计

### 当前架构
```
ai_settings (单行表, id=1) → Provider enum (claude/openai/ollama) → 单一模型配置
SettingsPanel: 手动填写 provider/model/base_url/api_key
AiPanel: 无模型切换，使用 ai_settings 中保存的唯一配置
```

### 目标架构
```
ai_models (多行表) → 多个模型配置（含内置预设 + 用户自定义）
ai_settings.active_model_id → 指向当前激活的模型
AiPanel 底部: 模型切换下拉框 → 切换 active_model_id → 下次对话使用新模型
```

## 数据模型

### 新增 `ai_models` 表
```sql
CREATE TABLE IF NOT EXISTS ai_models (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL,           -- 显示名: "GLM Coding Plan", "我的 Claude"
    provider     TEXT NOT NULL,           -- "claude" | "openai" | "openai_compatible" | "anthropic_compatible"
    model_id     TEXT NOT NULL,           -- 模型标识: "glm-4", "MiniMax-M3"
    base_url     TEXT,                    -- 自定义 base URL
    api_key_enc  TEXT,                    -- AES-256-GCM 加密
    proxy_url    TEXT,
    temperature  REAL NOT NULL DEFAULT 0.7,
    is_preset    INTEGER NOT NULL DEFAULT 0,  -- 1=内置预设, 0=用户创建
    sort_order   INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### 修改 `ai_settings` 表
```sql
ALTER TABLE ai_settings ADD COLUMN active_model_id INTEGER REFERENCES ai_models(id);
```
- 保留原有字段作为 fallback（向后兼容旧版数据）
- `active_model_id` 指向 `ai_models.id`，优先使用

### Provider 扩展
在 `ai.rs` 的 `Provider` enum 中新增：
- `OpenAiCompatible` — 使用 OpenAI 协议，自定义 base_url
- `AnthropicCompatible` — 使用 Anthropic 协议，自定义 base_url

两者的请求格式分别复用现有 `OpenAi` / `Claude` 的 `build_body` / `auth_headers` / `token_from_line`，只是 base_url 可自定义。

## 内置预设列表

| 名称 | Provider | Model ID | Base URL |
|------|----------|----------|----------|
| GLM Coding Plan (OpenAI) | openai_compatible | glm-4 | https://open.bigmodel.cn/api/coding/paas/v4 |
| GLM Coding Plan (Anthropic) | anthropic_compatible | glm-4 | https://open.bigmodel.cn/api/anthropic |
| MIMO 按量付费 (OpenAI) | openai_compatible | mimo-default | https://api.xiaomimimo.com/v1 |
| MIMO 按量付费 (Anthropic) | anthropic_compatible | mimo-default | https://api.xiaomimimo.com/anthropic |
| MiniMax M3 (OpenAI) | openai_compatible | MiniMax-M3 | https://api.minimaxi.com/v1 |
| MiniMax M3 (Anthropic) | anthropic_compatible | MiniMax-M3 | https://api.minimaxi.com/anthropic |
| LongCat (OpenAI) | openai_compatible | longcat-default | https://api.longcat.chat/openai |
| LongCat (Anthropic) | anthropic_compatible | longcat-default | https://api.longcat.chat/anthropic |
| DeepSeek | openai_compatible | deepseek-chat | https://api.deepseek.com/v1 |
| 通义千问 (阿里云) | openai_compatible | qwen-turbo | https://dashscope.aliyuncs.com/compatible-mode/v1 |
| 混元 (腾讯云) | openai_compatible | hunyuan-lite | https://api.hunyuan.cloud.tencent.com/v1 |
| Claude (原有) | claude | claude-sonnet-4-20250514 | (默认) |
| OpenAI (原有) | openai | gpt-4o | (默认) |
| Ollama (原有) | ollama | llama3 | http://localhost:11434/api |

## 改动清单

### 1. Rust 后端 (`src-tauri/src/ai.rs`)
- **Provider enum**: 新增 `OpenAiCompatible`, `AnthropicCompatible`
- **Provider 实现**: 两个新 variant 复用 `OpenAi` / `Claude` 的逻辑，只是 `endpoint()` 使用自定义 base_url
- **LoadedSettings**: 改为从 `ai_models` 表读取（按 `active_model_id`），fallback 到旧 `ai_settings`
- **新增 Tauri commands**:
  - `list_ai_models` → 返回所有模型列表（不含 api_key）
  - `save_ai_model(name, provider, model_id, base_url, api_key, proxy_url, temperature)` → 新增/更新模型
  - `delete_ai_model(id)` → 删除用户模型（不可删预设）
  - `set_active_ai_model(id)` → 切换当前激活模型
  - `init_ai_presets` → 首次启动时插入内置预设（如不存在）

### 2. Rust 后端 (`src-tauri/src/db.rs`)
- `ai_models` 表 DDL
- `ai_settings` 表 ALTER 添加 `active_model_id` 列
- 预设数据插入（幂等，用 `INSERT OR IGNORE`）

### 3. Rust 后端 (`src-tauri/src/main.rs`)
- 注册新 commands 到 `generate_handler!`

### 4. 前端类型 (`src/api.ts`)
- 新增 `AiModel` 接口
- 新增 `AiModelPreset` 类型
- 新增 API wrappers: `listAiModels`, `saveAiModel`, `deleteAiModel`, `setActiveAiModel`
- 扩展 `AiProvider` 类型添加 `"openai_compatible" | "anthropic_compatible"`

### 5. 前端 UI (`src/components/AiPanel.tsx`)
- **底部模型切换器**: 在消息输入框下方添加一个水平滚动的模型标签/chip 列表
  - 显示当前激活模型名称（高亮）
  - 点击切换到其他模型
  - 切换后下次对话使用新模型
  - 需要输入 API Key 的模型标红提示
- **首次使用引导**: 如果没有配置过任何模型的 API Key，提示用户去设置

### 6. 前端 UI (`src/components/SettingsPanel.tsx`)
- **AI 设置页改造**:
  - 新增"模型管理"区域，列出所有已配置模型 + 预设
  - 每个模型可展开编辑（api_key, temperature, proxy）
  - 预设只需填 API Key 即可使用（base_url/model 已内置）
  - 用户可新增自定义模型（选择协议类型 + 填写完整配置）
  - 设为"默认"按钮 → 调用 `set_active_ai_model`

### 7. Hook (`src/hooks/useAiConfig.ts`)
- 扩展为同时管理 `AiModel[]` 列表和当前激活模型
- `reload()` 同时刷新模型列表

## 实现顺序

1. **DB + Rust**: `ai_models` 表 + Provider 扩展 + 新 commands + 预设初始化
2. **前端 API 层**: 类型定义 + IPC wrappers
3. **SettingsPanel**: 模型管理 UI（列表 + 编辑 + 新增 + 预设快捷配置）
4. **AiPanel**: 底部模型切换器
5. **useAiConfig hook**: 扩展数据流
6. **验证**: tsc + cargo check + 手动测试

## 安全
- API Key 始终加密存储（复用现有 `crypto::encrypt_with_key`）
- 前端永远看不到明文 Key（`hasKey` 字段）
- 预设模型的 Key 为空，用户必须自行填写

## 向后兼容
- 旧版 `ai_settings` 数据自动迁移：首次加载时检测无 `ai_models` 记录，将旧配置导入为第一个用户模型
- 保留旧 `ai_settings` 表结构，`active_model_id` 为可选列