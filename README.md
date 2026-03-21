# Desktop Pet 🐱

A simple desktop pet application.

## Features
- Animated desktop pet
- Drag to move
- Sound effects
- Custom skins

## Run

```bash
git clone https://github.com/qingchuanluo/Desktop-pet.git
cd Desktop-pet
```

## 后台 API（gateway-api）

默认后台绑定地址由环境变量 `BACKEND_BIND` 控制，未设置时为 `127.0.0.1:4317`。

约定：
- 除图片接口与 WebSocket 外，均为 `application/json`
- `API_BASE = http://127.0.0.1:4317`

### 配置

#### GET /api/config
返回当前配置（不会返回 `api_key`，只返回是否已设置）。

响应：
```json
{
  "bind": "127.0.0.1:4317",
  "base_url": "https://api.deepseek.com/v1",
  "model": "deepseek-chat",
  "system_prompt": "你是桌宠的聊天助手，回答简洁自然。",
  "api_key_set": true
}
```

#### POST /api/config
更新配置（字段可选）。

请求：
```json
{
  "bind": "127.0.0.1:4317",
  "base_url": "https://api.deepseek.com/v1",
  "model": "deepseek-chat",
  "system_prompt": "…",
  "api_key": "sk-xxx"
}
```

响应：同 `GET /api/config`

### 人设（当前注入）

#### GET /api/persona/current
返回最近一次通过 WebSocket 注入的人设与时间戳（可能为空）。

响应：
```json
{
  "ts_ms": 0,
  "personality": null
}
```

### 日志/监控/进程

#### GET /api/logs
响应：数组
```json
[
  {"ts_ms": 1710000000000, "level": "info", "message": "..." }
]
```

#### GET /api/monitor
响应：监控信息（JSON 对象，字段随实现变化）

#### GET /api/processes?limit=200
响应：
```json
{
  "total": 123,
  "processes": [
    {"pid": 1, "name": "System", "cpu_usage": 0.0, "memory_kb": 0 }
  ]
}
```

### 记忆

#### GET /api/memory
响应：
```json
{
  "path": "memory/memory.json",
  "data": {}
}
```

#### POST /api/memory/clear
响应：
```json
{"ok": true}
```

### 宠物等级/饥饿/金币

#### GET /api/pet_level
响应：
```json
{"level":0,"xp":0,"xp_to_next":50,"hunger":100,"coins":0}
```

#### POST /api/pet_level
请求（字段可选）：
```json
{"level":2,"xp":10,"hunger":80,"coins":120}
```
响应：
```json
{"ok": true}
```

说明：
- `level`：允许为 `0`（重置后为 0）
- `xp_to_next`：固定为 `50`
- `hunger`：后台会 clamp 到 `0..=100`
- `coins`：可选；若不传 `coins` 且 `level` 增加，后台会按升级差值自动加金币（当前为 `+10/级`）

### 已安装角色与皮肤（本地 assets 扫描）

#### GET /api/owned/items
响应：
```json
{
  "items": [
    {
      "id": "dusk",
      "name": "Dusk",
      "description": "…",
      "cover": "/api/owned/item-covers/dusk",
      "skins_count": 3
    }
  ]
}
```

#### GET /api/owned/items/:item_id/skins
响应：
```json
{
  "item": {
    "id": "dusk",
    "name": "Dusk",
    "description": "…",
    "cover": "/api/owned/item-covers/dusk",
    "skins_count": 3
  },
  "skins": [
    {"name": "default", "cover": "/api/owned/covers/dusk/default"}
  ]
}
```

#### POST /api/owned/items/:item_id/delete
删除本地 `frontend/assets/:item_id` 目录下的角色模组（递归删除目录）。

响应（成功）：
```json
{"ok": true}
```

响应（失败示例）：
```json
{"ok": false, "error": "默认角色不可删除"}
```

说明：
- 默认角色 `debug_pet` 与 `夕` 不允许删除（接口会拒绝，前端按钮也会禁用）

#### GET /api/owned/item-covers/:item_id
返回角色封面图片（二进制）

#### GET /api/owned/covers/:item_id/:skin_name
返回皮肤封面图片（二进制）

### 商城代理与安装

#### GET /api/store/items?base=https://your-store.example.com
将请求代理到：`{base}/api/store/items`

响应：透传商城 JSON
说明：
- 若请求带 `Authorization` 头（例如 `Authorization: Bearer uid:alice`），后台会把该头透传给商城服务，用于联调账号/金币鉴权。

#### POST /api/store/install/start
请求：
```json
{
  "store_base_url": "https://your-store.example.com",
  "item_id": "dusk",
  "skin_name": "default"
}
```
响应：
```json
{"job_id":"inst_1710000000000_1"}
```
说明：
- 若请求带 `Authorization` 头，后台会把该头透传给商城的 package 下载请求（`/api/store/packages/...`）。

#### GET /api/store/install/status?job_id=inst_...
响应：
```json
{
  "job_id":"inst_...",
  "stage":"downloading",
  "downloaded_bytes": 123,
  "total_bytes": 456,
  "percent": 27.0,
  "done": false,
  "error": null,
  "installed_character_id": null
}
```

### 商城账号/金币（由“桌宠商城服务”提供）

说明：
- `gateway-api` 目前只做“商城列表/安装包下载”的代理与安装，不负责账号体系
- 账号登录、余额（金币）、扣费/购买等接口应由“桌宠商城服务（store_base_url）”提供

常见接口约定（示例，具体以你的商城实现为准）：

#### POST {store_base_url}/api/auth/login
用途：登录换取 token/session。

请求示例：
```json
{"username":"alice","password":"***"}
```

响应示例：
```json
{"ok":true,"token":"Bearer xxx","user":{"id":"u_1","name":"alice"}}
```

#### GET {store_base_url}/api/me
用途：获取当前登录用户信息（含金币余额/钱包信息等）。

响应示例：
```json
{"ok":true,"user":{"id":"u_1","name":"alice","coins":1200}}
```

#### GET {store_base_url}/api/wallet/balance
用途：获取金币余额。

响应示例：
```json
{"ok":true,"coins":1200}
```

#### POST {store_base_url}/api/store/purchase
用途：购买某个商品（角色/皮肤/资源包），成功后应把该商品加入“已拥有”，并允许下载对应 package。

请求示例：
```json
{"item_id":"dusk","skin_name":"default"}
```

响应示例：
```json
{"ok":true,"coins":1100,"owned":true}
```

#### GET {store_base_url}/api/store/items
用途：商品列表（已在 gateway 侧通过 `/api/store/items` 代理）。

#### GET {store_base_url}/api/store/packages/{item_id}
用途：下载该商品的完整资源包（zip）。

#### GET {store_base_url}/api/store/packages/{item_id}/{skin_name}
用途：下载该商品指定皮肤的资源包（zip）。

### 日记

#### GET /api/diary
响应：
```json
{
  "path": "memory/diary.json",
  "data": {}
}
```

#### POST /api/diary/append
请求：
```json
{"text":"今天很开心"}
```
响应：同 `GET /api/diary`

#### POST /api/diary/clear
响应：
```json
{"ok": true}
```

#### POST /api/diary/summarize
请求：任意 JSON（目前不使用请求体）
响应：
```json
{"summary":"…","error":null}
```

#### POST /api/diary/auto_processes
请求：
```json
{"character_id":"dusk","max_processes":25}
```
响应：
```json
{"ok":true,"entry":{},"error":null}
```

### 文件摘要

#### POST /api/file/summarize
请求：
```json
{"path":"E:/Desktop-pet/README.md"}
```
响应：
```json
{"ok":true,"summary":"…","error":null}
```

### 自动聊天

#### POST /api/auto_talk
响应：
```json
{"ok":true,"text":"…","error":null}
```

#### WebSocket /ws/auto_talk
服务端推送：
```json
{
  "event":"auto_talk",
  "source":"timer",
  "ok":true,
  "level":null,
  "text":"…",
  "stats":null,
  "error":null,
  "timestamp":1710000000
}
```

客户端可发送（Text JSON 或 Binary bincode，同字段）：
```json
{"event":"pet_clicked","personality":{ "name":"..." }}
```
```json
{"event":"level_up","level":2}
```
```json
{"event":"feed","delta":25,"text":"博士，谢谢款待~"}
```

### 对话测试/聊天

#### POST /api/test
请求：
```json
{"text":"你好","personality":null}
```
响应：
```json
{"ok":true,"reply":"…"}
```

#### POST /api/chat
请求：
```json
{"text":"你好","personality":{"name":"dusk"}}
```
响应：
```json
{"ok":true,"reply":"…","error":null}
```
