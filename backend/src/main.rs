//! AI 桌宠后台服务 - 主入口
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    http::StatusCode,
    response::Html,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chat_service::{build_chat_messages, build_test_message, call_ai_openai_compat, AiConfig};
use gateway_api::state::{
    ApiOk, AppState, BackendChatReq, BackendChatResp, BackendConfig, BackendConfigPublic,
    BackendConfigUpdate, BackendLog, BackendTestResp, DiaryAppendReq, DiaryResp,
    DiarySummarizeResp, MemoryResp, PetLevel,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn push_log(state: &Arc<AppState>, level: &str, message: String) {
    if let Ok(mut logs) = state.logs.lock() {
        logs.push(BackendLog {
            ts_ms: now_ms(),
            level: level.to_string(),
            message,
        });
        if logs.len() > 300 {
            let drain_count = logs.len() - 300;
            logs.drain(0..drain_count);
        }
    }
}

async fn index() -> Html<String> {
    let html = r#"<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
  <title>AI 桌宠 - 管理中心</title>
    <style>
      :root {
      --bg: #0d0d12;
      --surface: #16161d;
      --surface-hover: #1e1e28;
      --border: #2a2a38;
      --primary: #845ef7;
      --primary-hover: #9771f8;
      --accent: #4cc5ff;
      --text: #e8e8ed;
      --text-dim: #8888a0;
      --success: #40c057;
      --danger: #ff6b6b;
    }
    * { margin: 0; padding: 0; box-sizing: border-box; }
      body {
      font-family: "Segoe UI", system-ui, sans-serif;
      background: var(--bg);
      color: var(--text);
      min-height: 100vh;
    }
    .layout { display: flex; min-height: 100vh; }
    
    /* 侧边栏 */
    .sidebar {
        width: 220px;
      background: var(--surface);
      border-right: 1px solid var(--border);
      padding: 20px 0;
      flex-shrink: 0;
    }
    .logo {
      padding: 0 20px 20px;
      border-bottom: 1px solid var(--border);
      margin-bottom: 20px;
    }
    .logo h1 { font-size: 18px; font-weight: 600; color: var(--text); }
    .logo p { font-size: 12px; color: var(--text-dim); margin-top: 4px; }
    .nav-item {
        display: flex;
        align-items: center;
      gap: 12px;
      padding: 12px 20px;
      color: var(--text-dim);
        cursor: pointer;
      transition: all 0.15s;
      border-left: 3px solid transparent;
    }
    .nav-item:hover { background: var(--surface-hover); color: var(--text); }
    .nav-item.active { 
      background: rgba(132, 94, 247, 0.1); 
      color: var(--primary);
      border-left-color: var(--primary);
    }
    .nav-icon { font-size: 18px; width: 24px; text-align: center; }
    .nav-label { font-size: 14px; }
    
    /* 主内容区 */
    .main { flex: 1; padding: 30px; overflow-y: auto; }
      .page { display: none; }
    .page.active { display: block; }
    .page-header { margin-bottom: 24px; }
    .page-title { font-size: 24px; font-weight: 600; margin-bottom: 8px; }
    .page-desc { color: var(--text-dim); font-size: 14px; }
    
    /* 卡片 */
      .card {
      background: var(--surface);
      border: 1px solid var(--border);
      border-radius: 12px;
      padding: 20px;
      margin-bottom: 16px;
    }
    .card-title { font-size: 14px; font-weight: 600; margin-bottom: 16px; color: var(--text); }
    
    /* 表单 */
    .form-group { margin-bottom: 16px; }
    .form-label { 
      display: block; 
      font-size: 13px; 
      color: var(--text-dim); 
      margin-bottom: 6px; 
    }
    .form-input {
        width: 100%;
      padding: 10px 14px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 8px;
        color: var(--text);
      font-size: 14px;
        outline: none;
      transition: border-color 0.15s;
    }
    .form-input:focus { border-color: var(--primary); }
    .form-input::placeholder { color: var(--text-dim); }
    textarea.form-input { min-height: 100px; resize: vertical; }
    
    /* 按钮 */
    .btn {
      padding: 10px 20px;
      border-radius: 8px;
      font-size: 14px;
      font-weight: 500;
      cursor: pointer;
      border: none;
      transition: all 0.15s;
    }
    .btn-primary { 
      background: var(--primary); 
      color: white; 
    }
    .btn-primary:hover { background: var(--primary-hover); }
    .btn-secondary { 
      background: var(--surface-hover); 
        color: var(--text);
      border: 1px solid var(--border);
    }
    .btn-secondary:hover { border-color: var(--text-dim); }
    .btn-danger { 
      background: rgba(255, 107, 107, 0.15); 
      color: var(--danger);
      border: 1px solid rgba(255, 107, 107, 0.3);
    }
    .btn-danger:hover { background: rgba(255, 107, 107, 0.25); }
    
    .btn-row { display: flex; gap: 10px; margin-top: 20px; }
    
    /* 状态指示 */
    .status {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 4px 10px;
      border-radius: 20px;
      font-size: 12px;
    }
    .status-dot {
      width: 8px;
      height: 8px;
      border-radius: 50%;
    }
    .status-on { background: rgba(64, 192, 87, 0.2); color: var(--success); }
    .status-on .status-dot { background: var(--success); }
    .status-off { background: rgba(255, 107, 107, 0.2); color: var(--danger); }
    .status-off .status-dot { background: var(--danger); }
    
    /* 列表 */
    .diary-list { max-height: 300px; overflow-y: auto; }
    .diary-item {
      padding: 12px;
      background: var(--bg);
      border-radius: 8px;
      margin-bottom: 8px;
    }
    .diary-time { font-size: 11px; color: var(--text-dim); margin-bottom: 4px; }
    .diary-text { font-size: 13px; line-height: 1.5; }
    
    /* 测试结果 */
    .test-result {
      margin-top: 16px;
      padding: 12px;
      background: var(--bg);
      border-radius: 8px;
      font-size: 13px;
      display: none;
    }
    .test-result.show { display: block; }
    .test-result.success { border-left: 3px solid var(--success); }
    .test-result.error { border-left: 3px solid var(--danger); }
    
    /* 监控数据 */
    .monitor-grid {
      display: grid;
      grid-template-columns: repeat(2, 1fr);
      gap: 12px;
    }
    .monitor-item {
      background: var(--bg);
      padding: 12px;
      border-radius: 8px;
    }
    .monitor-label { font-size: 11px; color: var(--text-dim); }
    .monitor-value { font-size: 20px; font-weight: 600; margin-top: 4px; }
    
    /* 宠物个性预览 */
    .persona-preview {
      background: linear-gradient(135deg, rgba(132, 94, 247, 0.1), rgba(76, 197, 255, 0.1));
      border: 1px solid var(--border);
        border-radius: 12px;
      padding: 20px;
      margin-bottom: 20px;
    }
    .persona-avatar {
      width: 60px;
      height: 60px;
      border-radius: 50%;
      background: linear-gradient(135deg, var(--primary), var(--accent));
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 28px;
      margin-bottom: 12px;
    }
    .persona-name { font-size: 16px; font-weight: 600; }
    .persona-trait { font-size: 12px; color: var(--accent); margin-top: 4px; }
    </style>
  </head>
  <body>
    <div class="layout">
    <!-- 侧边栏 -->
    <div class="sidebar">
      <div class="logo">
        <h1>🐾 桌宠管理中心</h1>
        <p>v0.1.0</p>
      </div>
      <div class="nav-item active" data-page="api">
        <span class="nav-icon">🔗</span>
        <span class="nav-label">API 绑定</span>
      </div>
      <div class="nav-item" data-page="persona">
        <span class="nav-icon">🎭</span>
        <span class="nav-label">宠物个性</span>
      </div>
      <div class="nav-item" data-page="characters">
        <span class="nav-icon">🧩</span>
        <span class="nav-label">角色库</span>
      </div>
      <div class="nav-item" data-page="diary">
        <span class="nav-icon">📔</span>
        <span class="nav-label">宠物日记</span>
      </div>
      <div class="nav-item" data-page="monitor">
        <span class="nav-icon">📊</span>
        <span class="nav-label">系统监控</span>
      </div>
    </div>
    
    <!-- 主内容 -->
    <div class="main">
      <!-- API 绑定页面 -->
          <div class="page active" id="page-api">
        <div class="page-header">
          <h2 class="page-title">🔗 API 绑定设置</h2>
          <p class="page-desc">配置 AI 服务商的 API 连接信息</p>
        </div>
        
            <div class="card">
          <div class="card-title">连接状态</div>
          <div id="api-status">
            <span class="status status-off"><span class="status-dot"></span>未配置</span>
          </div>
            </div>

            <div class="card">
          <div class="card-title">API 配置</div>
          <div class="form-group">
            <label class="form-label">Base URL</label>
            <input type="text" class="form-input" id="api-base-url" placeholder="https://api.deepseek.com/v1">
                </div>
          <div class="form-group">
            <label class="form-label">模型名称</label>
            <input type="text" class="form-input" id="api-model" placeholder="deepseek-chat">
                </div>
          <div class="form-group">
            <label class="form-label">API Key</label>
            <input type="password" class="form-input" id="api-key" placeholder="sk-xxxxxxxx">
                </div>
          <div class="btn-row">
            <button class="btn btn-primary" onclick="saveApiConfig()">保存配置</button>
            <button class="btn btn-secondary" onclick="testApi()">测试连接</button>
                </div>
          <div class="test-result" id="test-result"></div>
              </div>
              </div>
      
      <!-- 宠物个性页面 -->
      <div class="page" id="page-persona">
        <div class="page-header">
          <h2 class="page-title">🎭 宠物个性设置</h2>
          <p class="page-desc">定义宠物的性格、语气和行为模式</p>
        </div>
        
        <div class="persona-preview">
          <div class="persona-avatar">🐱</div>
          <div class="persona-name" id="persona-name-preview">可爱的小猫</div>
          <div class="persona-trait" id="persona-trait-preview">活泼 · 好奇 · 黏人</div>
            </div>

            <div class="card">
          <div class="card-title">基本设置</div>
          <div class="form-group">
            <label class="form-label">宠物名称</label>
            <input type="text" class="form-input" id="persona-name" placeholder="可爱的小猫" oninput="updatePreview()">
              </div>
          <div class="form-group">
            <label class="form-label">宠物类型</label>
            <select class="form-input" id="persona-type" onchange="updatePreview()">
              <option value="cat">🐱 猫咪</option>
              <option value="dog">🐕 小狗</option>
              <option value="fox">🦊 狐狸</option>
              <option value="rabbit">🐰 兔子</option>
              <option value="dragon">🐉 龙</option>
              <option value="robot">🤖 机器人</option>
            </select>
          </div>
          <div class="form-group">
            <label class="form-label">性格标签（用 · 分隔）</label>
            <input type="text" class="form-input" id="persona-traits" placeholder="活泼 · 好奇 · 黏人" oninput="updatePreview()">
          </div>
            </div>

            <div class="card">
          <div class="card-title">AI 角色设定</div>
          <div class="form-group">
            <label class="form-label">系统提示词</label>
            <textarea class="form-input" id="system-prompt" rows="4" placeholder="你是主人的贴心小宠物，活泼可爱，喜欢撒娇..."></textarea>
                </div>
              </div>
        
        <div class="btn-row">
          <button class="btn btn-primary" onclick="savePersona()">保存个性</button>
          <button class="btn btn-secondary" onclick="resetPersona()">重置默认</button>
        </div>
      </div>

      <div class="page" id="page-characters">
        <div class="page-header">
          <h2 class="page-title">🧩 角色库</h2>
          <p class="page-desc">管理与安装角色模组（支持热切换）</p>
        </div>

        <div class="card">
          <div class="card-title">添加其他角色</div>
          <p style="color: var(--text-dim); font-size: 13px; margin: 0 0 12px 0;">
            点击后输入你的服务器地址，后续你接入下载服务后即可使用。
          </p>
          <div class="btn-row" style="margin-top: 0;">
            <button class="btn btn-primary" onclick="openRoleStore()">增加其他角色</button>
          </div>
        </div>
      </div>
      
      <!-- 宠物日记页面 -->
      <div class="page" id="page-diary">
        <div class="page-header">
          <h2 class="page-title">📔 宠物日记</h2>
          <p class="page-desc">记录你和宠物的日常互动，AI 会自动总结</p>
            </div>

            <div class="card">
          <div class="card-title">记录新日记</div>
          <div class="form-group">
            <textarea class="form-input" id="diary-input" rows="3" placeholder="今天主人和我一起看了电影..."></textarea>
              </div>
          <div class="btn-row">
            <button class="btn btn-primary" onclick="addDiary()">添加记录</button>
            <button class="btn btn-secondary" onclick="summarizeDiary()">AI 总结</button>
            </div>
          </div>

            <div class="card">
          <div class="card-title">日记历史</div>
          <div class="btn-row" style="margin-top: 0; margin-bottom: 16px;">
            <button class="btn btn-secondary" onclick="loadDiary()">刷新</button>
            <button class="btn btn-danger" onclick="clearDiary()">清空</button>
            </div>
          <div class="diary-list" id="diary-list">
            <p style="color: var(--text-dim); font-size: 13px;">暂无日记记录</p>
              </div>
              </div>
        
        <div class="card" id="summary-card" style="display: none;">
          <div class="card-title">📝 AI 总结</div>
          <div id="diary-summary"></div>
            </div>
          </div>

      <!-- 系统监控页面 -->
      <div class="page" id="page-monitor">
        <div class="page-header">
          <h2 class="page-title">📊 系统监控</h2>
          <p class="page-desc">实时查看系统资源使用情况</p>
            </div>
        
            <div class="card">
          <div class="card-title">资源使用</div>
          <div class="monitor-grid">
            <div class="monitor-item">
              <div class="monitor-label">CPU 使用率</div>
              <div class="monitor-value" id="cpu-usage">--%</div>
                </div>
            <div class="monitor-item">
              <div class="monitor-label">内存使用</div>
              <div class="monitor-value" id="mem-usage">--</div>
              </div>
            <div class="monitor-item">
              <div class="monitor-label">后台进程内存</div>
              <div class="monitor-value" id="self-mem-usage">--</div>
              </div>
            <div class="monitor-item">
              <div class="monitor-label">进程数</div>
              <div class="monitor-value" id="proc-count">--</div>
              </div>
            <div class="monitor-item">
              <div class="monitor-label">后台运行</div>
              <div class="monitor-value" id="bg-task">10分钟</div>
              </div>
            </div>
          <div class="btn-row">
            <button class="btn btn-secondary" onclick="refreshMonitor()">刷新数据</button>
          </div>
        </div>
      </div>
    </div>
    </div>

    <script>
    const API_BASE = '';
    
    // 导航切换
    document.querySelectorAll('.nav-item').forEach(item => {
      item.addEventListener('click', () => {
        document.querySelectorAll('.nav-item').forEach(i => i.classList.remove('active'));
        document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
        item.classList.add('active');
        document.getElementById('page-' + item.dataset.page).classList.add('active');
        if (item.dataset.page === 'api') loadApiConfig();
        if (item.dataset.page === 'diary') loadDiary();
        if (item.dataset.page === 'monitor') refreshMonitor();
      });
    });

    const initPage = new URLSearchParams(window.location.search).get('page');
    if (initPage) {
      const el = document.querySelector(`.nav-item[data-page="${initPage}"]`);
      if (el) el.click();
    }
    
    // 加载 API 配置
    async function loadApiConfig() {
      try {
        const res = await fetch(API_BASE + '/api/config');
        const data = await res.json();
        document.getElementById('api-base-url').value = data.base_url || '';
        document.getElementById('api-model').value = data.model || '';
        document.getElementById('api-key').value = '';
        document.getElementById('system-prompt').value = data.system_prompt || '';
        
        const statusEl = document.getElementById('api-status');
        if (data.api_key_set) {
          statusEl.innerHTML = '<span class="status status-on"><span class="status-dot"></span>已配置</span>';
        } else {
          statusEl.innerHTML = '<span class="status status-off"><span class="status-dot"></span>未配置</span>';
        }
      } catch (e) {
        console.error(e);
      }
    }
    
    // 保存 API 配置
    async function saveApiConfig() {
      const base_url = document.getElementById('api-base-url').value;
      const model = document.getElementById('api-model').value;
      const api_key = document.getElementById('api-key').value;
      const system_prompt = document.getElementById('system-prompt').value;
      
      const body = {};
      if (base_url) body.base_url = base_url;
      if (model) body.model = model;
      if (api_key) body.api_key = api_key;
      if (system_prompt) body.system_prompt = system_prompt;
      
      try {
        const res = await fetch(API_BASE + '/api/config', {
          method: 'POST',
          headers: {'Content-Type': 'application/json'},
          body: JSON.stringify(body)
        });
        const data = await res.json();
        if (data.api_key_set) {
          document.getElementById('api-status').innerHTML = '<span class="status status-on"><span class="status-dot"></span>已配置</span>';
        }
        alert('配置已保存！');
      } catch (e) {
        alert('保存失败: ' + e);
      }
    }
    
    // 测试 API
    async function testApi() {
      const resultEl = document.getElementById('test-result');
      resultEl.className = 'test-result show';
      resultEl.textContent = '正在测试...';
      
      try {
        const res = await fetch(API_BASE + '/api/test', {
          method: 'POST',
          headers: {'Content-Type': 'application/json'},
          body: JSON.stringify({text: '你好'})
        });
        const data = await res.json();
        if (data.ok) {
          resultEl.className = 'test-result show success';
          resultEl.textContent = '✓ 连接成功！AI 回复: ' + data.reply;
        } else {
          resultEl.className = 'test-result show error';
          resultEl.textContent = '✗ 连接失败: ' + (data.error || '未知错误');
        }
      } catch (e) {
        resultEl.className = 'test-result show error';
        resultEl.textContent = '✗ 请求失败: ' + e;
      }
    }

    function openRoleStore() {
      const url = window.prompt('输入你的角色下载服务器地址', 'https://');
      if (!url) return;
      const u = url.trim();
      if (!u) return;
      window.open(u, '_blank');
    }
    
    // 更新个性预览
    function updatePreview() {
      const typeEmojis = {cat:'🐱', dog:'🐕', fox:'🦊', rabbit:'🐰', dragon:'🐉', robot:'🤖'};
      const name = document.getElementById('persona-name').value || '可爱的小猫';
      const type = document.getElementById('persona-type').value;
      const traits = document.getElementById('persona-traits').value || '活泼 · 好奇 · 黏人';
      
      document.querySelector('.persona-avatar').textContent = typeEmojis[type];
      document.getElementById('persona-name-preview').textContent = name;
      document.getElementById('persona-trait-preview').textContent = traits;
    }
    
    // 保存个性
    async function savePersona() {
      await saveApiConfig();
      alert('个性设置已保存！');
    }
    
    // 重置个性
    function resetPersona() {
      document.getElementById('persona-name').value = '可爱的小猫';
      document.getElementById('persona-type').value = 'cat';
      document.getElementById('persona-traits').value = '活泼 · 好奇 · 黏人';
      document.getElementById('system-prompt').value = '你是主人的贴心小宠物，活泼可爱，喜欢撒娇。';
      updatePreview();
    }
    
    // 加载日记
    async function loadDiary() {
      try {
        const res = await fetch(API_BASE + '/api/diary');
        const data = await res.json();
        const list = document.getElementById('diary-list');
        
        if (data.data.entries && data.data.entries.length > 0) {
          list.innerHTML = data.data.entries.slice().reverse().map(e => 
            '<div class="diary-item"><div class="diary-time">' + new Date(e.ts_ms).toLocaleString() + '</div><div class="diary-text">' + e.text + '</div></div>'
          ).join('');
        } else {
          list.innerHTML = '<p style="color: var(--text-dim); font-size: 13px;">暂无日记记录</p>';
        }
      } catch (e) {
        console.error(e);
      }
    }
    
    // 添加日记
    async function addDiary() {
      const text = document.getElementById('diary-input').value.trim();
      if (!text) return;
      
      try {
        await fetch(API_BASE + '/api/diary/append', {
          method: 'POST',
          headers: {'Content-Type': 'application/json'},
          body: JSON.stringify({text})
        });
        document.getElementById('diary-input').value = '';
        loadDiary();
      } catch (e) {
        alert('添加失败: ' + e);
      }
    }
    
    // 清空日记
    async function clearDiary() {
      if (!confirm('确定要清空所有日记吗？')) return;
      try {
        await fetch(API_BASE + '/api/diary/clear', {method: 'POST'});
        loadDiary();
        document.getElementById('summary-card').style.display = 'none';
      } catch (e) {
        alert('清空失败: ' + e);
      }
    }
    
    // AI 总结
    async function summarizeDiary() {
      try {
        const res = await fetch(API_BASE + '/api/diary/summarize', {
          method: 'POST',
          headers: {'Content-Type': 'application/json'},
          body: JSON.stringify({})
        });
        const data = await res.json();
        if (data.summary) {
          document.getElementById('summary-card').style.display = 'block';
          document.getElementById('diary-summary').textContent = data.summary;
        }
      } catch (e) {
        alert('总结失败: ' + e);
      }
    }
    
    // 刷新监控
    async function refreshMonitor() {
      try {
        const res = await fetch(API_BASE + '/api/monitor');
        const data = await res.json();
        document.getElementById('cpu-usage').textContent = data.cpu_usage.toFixed(1) + '%';
        const memGB = (data.memory_used / 1024 / 1024 / 1024).toFixed(1);
        const totalGB = (data.memory_total / 1024 / 1024 / 1024).toFixed(1);
        document.getElementById('mem-usage').textContent = memGB + ' / ' + totalGB + ' GB';
        const selfMB = (data.self_memory_used / 1024 / 1024).toFixed(1);
        document.getElementById('self-mem-usage').textContent = selfMB + ' MB';
        document.getElementById('proc-count').textContent = data.process_count;
      } catch (e) {
        console.error(e);
      }
    }
    
    // 初始化
    loadApiConfig();
    resetPersona();
  </script>
</body>
</html>"#;
    Html(html.to_string())
}

async fn get_config(State(state): State<Arc<AppState>>) -> Json<BackendConfigPublic> {
    let cfg = state.config.lock().unwrap();
    Json(BackendConfigPublic {
        bind: cfg.bind.clone(),
        base_url: cfg.base_url.clone(),
        model: cfg.model.clone(),
        system_prompt: cfg.system_prompt.clone(),
        api_key_set: cfg.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
    })
}

async fn post_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BackendConfigUpdate>,
) -> Json<BackendConfigPublic> {
    let mut cfg = state.config.lock().unwrap();
    if let Some(v) = req.bind {
        cfg.bind = v;
    }
    if let Some(v) = req.base_url {
        cfg.base_url = v;
    }
    if let Some(v) = req.model {
        cfg.model = v;
    }
    if let Some(v) = req.system_prompt {
        cfg.system_prompt = v;
    }
    if let Some(v) = req.api_key {
        cfg.api_key = if v.is_empty() { None } else { Some(v) };
    }
    Json(BackendConfigPublic {
        bind: cfg.bind.clone(),
        base_url: cfg.base_url.clone(),
        model: cfg.model.clone(),
        system_prompt: cfg.system_prompt.clone(),
        api_key_set: cfg.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
    })
}

async fn get_logs(State(state): State<Arc<AppState>>) -> Json<Vec<BackendLog>> {
    let logs = state.logs.lock().unwrap().clone();
    Json(logs)
}

async fn get_monitor(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let mut monitor = state.monitor.lock().unwrap();
    let data = monitor.get_data();
    Json(serde_json::to_value(data).unwrap_or_default())
}

async fn get_memory(State(state): State<Arc<AppState>>) -> Json<MemoryResp> {
    let mem = state.memory.lock().unwrap();
    Json(MemoryResp {
        path: "memory/memory.json".to_string(),
        data: serde_json::to_value(mem.snapshot()).unwrap_or_default(),
    })
}

#[derive(serde::Deserialize)]
struct PetLevelUpdate {
    level: Option<u32>,
    xp: Option<u32>,
    hunger: Option<i32>,
    coins: Option<u32>,
}

async fn get_pet_level(State(state): State<Arc<AppState>>) -> Json<PetLevel> {
    let v = state.pet_level.lock().unwrap().clone();
    Json(v)
}

async fn post_pet_level(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PetLevelUpdate>,
) -> (StatusCode, Json<ApiOk>) {
    let path = PetLevel::default_path();
    let ok = if let Ok(mut g) = state.pet_level.lock() {
        g.apply_update(req.level, req.xp, req.hunger, req.coins);
        g.save(&path).is_ok()
    } else {
        false
    };
    if !ok {
        push_log(
            &state,
            "error",
            format!("保存 pet_level 失败: {}", path.display()),
        );
    }
    (StatusCode::OK, Json(ApiOk { ok }))
}

async fn clear_memory(State(state): State<Arc<AppState>>) -> (StatusCode, Json<ApiOk>) {
    if let Ok(mut mem) = state.memory.lock() {
        mem.clear();
    }
    (StatusCode::OK, Json(ApiOk { ok: true }))
}

async fn get_diary(State(state): State<Arc<AppState>>) -> Json<DiaryResp> {
    let diary = state.diary.lock().unwrap();
    Json(DiaryResp {
        path: "memory/diary.json".to_string(),
        data: serde_json::to_value(diary.snapshot()).unwrap_or_default(),
    })
}

async fn post_diary_append(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DiaryAppendReq>,
) -> Json<DiaryResp> {
    let mut diary = state.diary.lock().unwrap();
    diary.append(req.text.clone());
    Json(DiaryResp {
        path: "memory/diary.json".to_string(),
        data: serde_json::to_value(diary.snapshot()).unwrap_or_default(),
    })
}

async fn clear_diary(State(state): State<Arc<AppState>>) -> (StatusCode, Json<ApiOk>) {
    if let Ok(mut diary) = state.diary.lock() {
        diary.clear();
    }
    (StatusCode::OK, Json(ApiOk { ok: true }))
}

async fn post_diary_summarize(
    State(state): State<Arc<AppState>>,
    Json(_req): Json<serde_json::Value>,
) -> Json<DiarySummarizeResp> {
    let cfg = state.config.lock().unwrap().clone();
    let diary = state.diary.lock().unwrap().snapshot();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);
    let mut content = String::new();
    for e in diary.entries.iter().rev().take(50) {
        content.push_str(&format!("[{}] {}\n", e.ts_ms, e.text));
    }
    let msgs = build_chat_messages(
        &cfg.system_prompt,
        &format!("请总结以下日记要点，50字内：\n{}", content),
    );
    match call_ai_openai_compat(&ai_cfg, &msgs).await {
        Ok(s) => Json(DiarySummarizeResp {
            summary: Some(s),
            error: None,
        }),
        Err(e) => Json(DiarySummarizeResp {
            summary: None,
            error: Some(e.to_string()),
        }),
    }
}

async fn post_test(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BackendChatReq>,
) -> Json<BackendTestResp> {
    let cfg = state.config.lock().unwrap().clone();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);
    let msgs = build_test_message(&req.text);

    let reply = call_ai_openai_compat(&ai_cfg, &msgs).await.ok();

    Json(BackendTestResp {
        ok: reply.is_some(),
        reply,
    })
}

async fn post_chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BackendChatReq>,
) -> Json<BackendChatResp> {
    let cfg = state.config.lock().unwrap().clone();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);
    if let Some(p) = req.personality.as_ref() {
        let persona_name = p.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
        push_log(&state, "info", format!("人设已注入: name={}", persona_name));
    }
    let system_prompt = system_prompt_with_personality(&cfg.system_prompt, req.personality.as_ref());
    let msgs = build_chat_messages(&system_prompt, &req.text);

    match call_ai_openai_compat(&ai_cfg, &msgs).await {
        Ok(reply) => {
            push_log(&state, "info", format!("用户: {}", req.text));
            push_log(&state, "info", format!("AI: {}", reply));
            if let Ok(mut mem) = state.memory.lock() {
                mem.apply_exchange(&req.text, &reply);
            }
            Json(BackendChatResp {
                ok: true,
                reply: Some(reply),
                error: None,
            })
        }
        Err(e) => {
            push_log(&state, "error", format!("AI 错误: {}", e));
            Json(BackendChatResp {
                ok: false,
                reply: None,
                error: Some(e.to_string()),
            })
        }
    }
}

fn system_prompt_with_personality(
    base: &str,
    personality: Option<&serde_json::Value>,
) -> String {
    let Some(p) = personality else {
        return base.to_string();
    };
    let persona_json = serde_json::to_string_pretty(p).unwrap_or_default();
    format!(
        "{}\n\n你正在扮演以下桌宠角色。回答要符合人设与语气，不要提及你在扮演。\n人设（JSON）：\n{}",
        base, persona_json
    )
}

#[derive(serde::Deserialize)]
struct FileSummarizeReq {
    path: String,
}

#[derive(serde::Serialize)]
struct FileSummarizeResp {
    ok: bool,
    summary: Option<String>,
    error: Option<String>,
}

async fn post_file_summarize(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FileSummarizeReq>,
) -> Json<FileSummarizeResp> {
    let cfg = state.config.lock().unwrap().clone();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);
    let content = std::fs::read_to_string(&req.path).unwrap_or_default();
    let mem_block = {
        let mem = state.memory.lock().unwrap();
        mem.build_memory_block("文件摘要")
    };
    let input = format!(
        "根据用户记忆与上下文，摘要此文件：\n{}\n---\n{}",
        mem_block, content
    );
    let msgs = build_chat_messages(&cfg.system_prompt, &input);
    match call_ai_openai_compat(&ai_cfg, &msgs).await {
        Ok(s) => {
            if let Ok(mut diary) = state.diary.lock() {
                diary.append(format!("文件摘要: {} => {}", req.path, s));
            }
            if let Ok(mut mem) = state.memory.lock() {
                mem.apply_user_message(&format!("文件摘要: {}", s));
            }
            Json(FileSummarizeResp {
                ok: true,
                summary: Some(s),
                error: None,
            })
        }
        Err(e) => Json(FileSummarizeResp {
            ok: false,
            summary: None,
            error: Some(e.to_string()),
        }),
    }
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(serde::Serialize)]
struct WsPetStats {
    hunger: i32,
}

#[derive(serde::Serialize)]
struct WsAutoTalkEvent {
    event: &'static str,
    source: &'static str,
    ok: bool,
    level: Option<u32>,
    text: String,
    stats: Option<WsPetStats>,
    error: Option<String>,
    timestamp: i64,
}

#[derive(serde::Deserialize)]
struct WsClientEvent {
    event: String,
    level: Option<u32>,
    delta: Option<i32>,
    text: Option<String>,
    #[serde(default)]
    personality: Option<serde_json::Value>,
}

fn env_flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim();
            !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false")
        }
        Err(_) => false,
    }
}

async fn ws_send_auto_talk_event(
    socket: &mut WebSocket,
    msg: &WsAutoTalkEvent,
    force_text: bool,
) -> bool {
    if !force_text {
        match bincode::serialize(msg) {
            Ok(bin) => {
                println!(
                    "[ws_auto_talk] send binary len={} source={} ok={}",
                    bin.len(),
                    msg.source,
                    msg.ok
                );
                return socket.send(Message::Binary(bin)).await.is_ok();
            }
            Err(e) => {
                println!("[ws_auto_talk] send binary serialize_err={}", e);
            }
        }
    } else {
        println!(
            "[ws_auto_talk] send text forced source={} ok={}",
            msg.source, msg.ok
        );
    }

    match serde_json::to_string(msg) {
        Ok(s) => {
            println!(
                "[ws_auto_talk] send text fallback len={} source={} ok={}",
                s.len(),
                msg.source,
                msg.ok
            );
            socket.send(Message::Text(s)).await.is_ok()
        }
        Err(e) => {
            println!("[ws_auto_talk] send text serialize_err={}", e);
            false
        }
    }
}

async fn generate_auto_talk_text(
    state: &Arc<AppState>,
    personality: Option<&serde_json::Value>,
) -> Result<String, String> {
    let cfg = state.config.lock().unwrap().clone();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);
    let mem_block = {
        let mem = state.memory.lock().unwrap();
        mem.build_memory_block("自治闲聊")
    };
    let mon = {
        let mut m = state.monitor.lock().unwrap();
        serde_json::to_string(&m.get_data()).unwrap_or_default()
    };
    let prompt = format!(
        "根据监控与记忆，生成一句简短自然的自言自语：\n{}\n{}",
        mem_block, mon
    );
    let system_prompt = system_prompt_with_personality(&cfg.system_prompt, personality);
    let msgs = build_chat_messages(&system_prompt, &prompt);
    call_ai_openai_compat(&ai_cfg, &msgs)
        .await
        .map_err(|e| e.to_string())
}

async fn generate_interaction_text(
    state: &Arc<AppState>,
    kind: &str,
    level: Option<u32>,
    personality: Option<&serde_json::Value>,
) -> Result<String, String> {
    let cfg = state.config.lock().unwrap().clone();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);
    let mem_block = {
        let mem = state.memory.lock().unwrap();
        mem.build_memory_block("互动")
    };
    let mon = {
        let mut m = state.monitor.lock().unwrap();
        serde_json::to_string(&m.get_data()).unwrap_or_default()
    };

    let hint = match kind {
        "pet_clicked" => "用户点击了桌宠，请用一句简短可爱自然的话回应（不超过15字）。",
        "level_up" => {
            let lv = level.unwrap_or(0);
            return Ok(format!("升级啦！现在是 {lv} 级"));
        }
        _ => "请用一句简短自然的话回应。",
    };
    let prompt = format!("{}\n{}\n{}", hint, mem_block, mon);
    let system_prompt = system_prompt_with_personality(&cfg.system_prompt, personality);
    let msgs = build_chat_messages(&system_prompt, &prompt);
    call_ai_openai_compat(&ai_cfg, &msgs)
        .await
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct AutoTalkResp {
    ok: bool,
    text: Option<String>,
    error: Option<String>,
}

async fn post_auto_talk(State(state): State<Arc<AppState>>) -> Json<AutoTalkResp> {
    match generate_auto_talk_text(&state, None).await {
        Ok(s) => Json(AutoTalkResp {
            ok: true,
            text: Some(s),
            error: None,
        }),
        Err(e) => Json(AutoTalkResp {
            ok: false,
            text: None,
            error: Some(e.to_string()),
        }),
    }
}

async fn ws_auto_talk(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_auto_talk_loop(socket, state))
}

async fn ws_auto_talk_loop(mut socket: WebSocket, state: Arc<AppState>) {
    let interval_ms = std::env::var("WS_AUTO_TALK_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(30_000);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(interval_ms));

    let mock_text = std::env::var("WS_AUTO_TALK_MOCK_TEXT")
        .ok()
        .filter(|s| !s.is_empty());
    let mut mock_fail_once = env_flag("WS_AUTO_TALK_MOCK_FAIL_ONCE");
    let mut force_text_once = env_flag("WS_AUTO_TALK_FORCE_TEXT_ONCE");
    let mut last_personality: Option<serde_json::Value> = None;

    let first = if mock_fail_once {
        mock_fail_once = false;
        Err("mock ai failed".to_string())
    } else if let Some(t) = &mock_text {
        Ok(t.clone())
    } else {
        generate_auto_talk_text(&state, last_personality.as_ref()).await
    };
    let first_msg = WsAutoTalkEvent {
        event: "auto_talk",
        source: "timer",
        ok: first.is_ok(),
        level: None,
        text: first.clone().unwrap_or_default(),
        stats: None,
        error: first.err(),
        timestamp: now_ts(),
    };
    let force_text = if force_text_once {
        force_text_once = false;
        true
    } else {
        false
    };
    if !ws_send_auto_talk_event(&mut socket, &first_msg, force_text).await {
        return;
    }

    interval.tick().await;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let text = if mock_fail_once {
                    mock_fail_once = false;
                    Err("mock ai failed".to_string())
                } else if let Some(t) = &mock_text {
                    Ok(t.clone())
                } else {
                    generate_auto_talk_text(&state, last_personality.as_ref()).await
                };
                let msg = WsAutoTalkEvent {
                    event: "auto_talk",
                    source: "timer",
                    ok: text.is_ok(),
                    level: None,
                    text: text.clone().unwrap_or_default(),
                    stats: None,
                    error: text.err(),
                    timestamp: now_ts(),
                };
                let force_text = if force_text_once {
                    force_text_once = false;
                    true
                } else {
                    false
                };
                if !ws_send_auto_talk_event(&mut socket, &msg, force_text).await {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(s))) => {
                        if let Ok(evt) = serde_json::from_str::<WsClientEvent>(&s) {
                            println!("[ws_auto_talk] recv event={} level={:?}", evt.event, evt.level);
                            if let Some(p) = evt.personality.as_ref() {
                                last_personality = Some(p.clone());
                            }
                            match evt.event.as_str() {
                                "persona" => {}
                                "pet_clicked" => {
                                    let persona = evt.personality.as_ref().or(last_personality.as_ref());
                                    let (ok, text, err) = match generate_interaction_text(&state, "pet_clicked", None, persona).await {
                                        Ok(t) => (true, t, None),
                                        Err(e) => (false, String::new(), Some(e)),
                                    };
                                    let resp = WsAutoTalkEvent {
                                        event: "auto_talk",
                                        source: "pet_clicked",
                                        ok,
                                        level: None,
                                        text,
                                        stats: None,
                                        error: err,
                                        timestamp: now_ts(),
                                    };
                                    let _ = ws_send_auto_talk_event(&mut socket, &resp, false).await;
                                }
                                "level_up" => {
                                    let lvl = evt.level.unwrap_or(0);
                                    let resp = WsAutoTalkEvent {
                                        event: "auto_talk",
                                        source: "level_up",
                                        ok: true,
                                        level: Some(lvl),
                                        text: String::new(),
                                        stats: None,
                                        error: None,
                                        timestamp: now_ts(),
                                    };
                                    let _ = ws_send_auto_talk_event(&mut socket, &resp, false).await;
                                }
                                "feed" => {
                                    let delta = evt.delta.unwrap_or(25);
                                    let (hunger, ok_saved) = if let Ok(mut g) = state.pet_level.lock() {
                                        let next = g.hunger + delta;
                                        g.apply_update(None, None, Some(next), None);
                                        let path = PetLevel::default_path();
                                        let ok = g.save(&path).is_ok();
                                        (g.hunger, ok)
                                    } else {
                                        (100, false)
                                    };
                                    let text = evt.text.unwrap_or_else(|| "博士，谢谢款待~".to_string());
                                    let resp = WsAutoTalkEvent {
                                        event: "auto_talk",
                                        source: "feed",
                                        ok: ok_saved,
                                        level: None,
                                        text,
                                        stats: Some(WsPetStats { hunger }),
                                        error: if ok_saved { None } else { Some("save_failed".to_string()) },
                                        timestamp: now_ts(),
                                    };
                                    let _ = ws_send_auto_talk_event(&mut socket, &resp, false).await;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Binary(bin))) => {
                        if let Ok(evt) = bincode::deserialize::<WsClientEvent>(&bin) {
                            println!("[ws_auto_talk] recv event={} level={:?}", evt.event, evt.level);
                            if let Some(p) = evt.personality.as_ref() {
                                last_personality = Some(p.clone());
                            }
                            match evt.event.as_str() {
                                "persona" => {}
                                "pet_clicked" => {
                                    let persona = evt.personality.as_ref().or(last_personality.as_ref());
                                    let (ok, text, err) = match generate_interaction_text(&state, "pet_clicked", None, persona).await {
                                        Ok(t) => (true, t, None),
                                        Err(e) => (false, String::new(), Some(e)),
                                    };
                                    let resp = WsAutoTalkEvent {
                                        event: "auto_talk",
                                        source: "pet_clicked",
                                        ok,
                                        level: None,
                                        text,
                                        stats: None,
                                        error: err,
                                        timestamp: now_ts(),
                                    };
                                    let _ = ws_send_auto_talk_event(&mut socket, &resp, false).await;
                                }
                                "level_up" => {
                                    let lvl = evt.level.unwrap_or(0);
                                    let resp = WsAutoTalkEvent {
                                        event: "auto_talk",
                                        source: "level_up",
                                        ok: true,
                                        level: Some(lvl),
                                        text: String::new(),
                                        stats: None,
                                        error: None,
                                        timestamp: now_ts(),
                                    };
                                    let _ = ws_send_auto_talk_event(&mut socket, &resp, false).await;
                                }
                                "feed" => {
                                    let delta = evt.delta.unwrap_or(25);
                                    let (hunger, ok_saved) = if let Ok(mut g) = state.pet_level.lock() {
                                        let next = g.hunger + delta;
                                        g.apply_update(None, None, Some(next), None);
                                        let path = PetLevel::default_path();
                                        let ok = g.save(&path).is_ok();
                                        (g.hunger, ok)
                                    } else {
                                        (100, false)
                                    };
                                    let text = evt.text.unwrap_or_else(|| "博士，谢谢款待~".to_string());
                                    let resp = WsAutoTalkEvent {
                                        event: "auto_talk",
                                        source: "feed",
                                        ok: ok_saved,
                                        level: None,
                                        text,
                                        stats: Some(WsPetStats { hunger }),
                                        error: if ok_saved { None } else { Some("save_failed".to_string()) },
                                        timestamp: now_ts(),
                                    };
                                    let _ = ws_send_auto_talk_event(&mut socket, &resp, false).await;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let bind = std::env::var("BACKEND_BIND").unwrap_or_else(|_| "127.0.0.1:4317".to_string());

    let state = Arc::new(AppState::new(BackendConfig {
        bind: bind.clone(),
        base_url: std::env::var("AI_BASE_URL")
            .unwrap_or_else(|_| "https://api.deepseek.com/v1".to_string()),
        model: std::env::var("AI_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string()),
        system_prompt: std::env::var("AI_SYSTEM")
            .unwrap_or_else(|_| "你是桌宠的聊天助手，回答简洁自然。".to_string()),
        api_key: std::env::var("AI_API_KEY").ok(),
    }));

    {
        let state2 = state.clone();
        tokio::spawn(async move {
            let path = PetLevel::default_path();
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                if let Ok(g) = state2.pet_level.lock() {
                    let _ = g.save(&path);
                }
            }
        });
    }

    tracing::info!("后台平台启动：http://{}", bind);

    let app = Router::new()
        .route("/", get(index))
        .route("/api/config", get(get_config).post(post_config))
        .route("/api/logs", get(get_logs))
        .route("/api/monitor", get(get_monitor))
        .route("/api/memory", get(get_memory))
        .route("/api/memory/clear", post(clear_memory))
        .route("/api/pet_level", get(get_pet_level).post(post_pet_level))
        .route("/api/diary", get(get_diary))
        .route("/api/diary/append", post(post_diary_append))
        .route("/api/diary/clear", post(clear_diary))
        .route("/api/diary/summarize", post(post_diary_summarize))
        .route("/api/file/summarize", post(post_file_summarize))
        .route("/api/auto_talk", post(post_auto_talk))
        .route("/ws/auto_talk", get(ws_auto_talk))
        .route("/api/test", post(post_test))
        .route("/api/chat", post(post_chat))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: std::net::SocketAddr = bind.parse().expect("无效的地址");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!("监听地址: http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}
