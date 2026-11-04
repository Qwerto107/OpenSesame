'use strict';

const BUILD_VERSION = '__BUILD_VERSION__';
console.info('%c OpenSesame Web %c ' + BUILD_VERSION + ' ',
  'background:#06b6d4;color:#04202a;padding:3px 0;border-radius:3px 0 0 3px;font-family:monospace',
  'background:#cffafe;color:#0e7490;padding:3px 0;border-radius:0 3px 3px 0;font-family:monospace');
console.info('%cCopyright © 2026 Qwerto107. All rights reserved.', 'color:#0e7490;font-family:monospace');

const themeKey = 'opensesame.appearance';
function applyTheme(preference) {
  const dark = preference === 'dark' || (preference === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
}
// 此脚本在样式表与正文解析前执行，避免浅色页面先绘制成深色。
let initialTheme = 'light';
try {
  const saved = localStorage.getItem(themeKey);
  if (['system', 'light', 'dark'].includes(saved)) initialTheme = saved;
} catch {}
applyTheme(initialTheme);

function startApp() {
  const app = document.querySelector('#app');
  const dialog = document.querySelector('#dialog');
  let data = null;
  let auth = null;
  let page = 'servers';
  let currentId = null;
  let settingsDraft = null;
  let batchIds = [];
  let batchPreviousRuns = new Map();
  let polling = false;
  let autoChecking = false;
  const lastAutoCheck = new Map();
  let lastSync = 0;
  let pollTimer;
  let draftRevision = 0;
  let modalRevision = 0;
  let menuAnchor = null;
  let progressSource = null;
  let progressReady = null;
  let pendingKnocks = 0;
  const startingKnocks = new Map();
  const media = window.matchMedia('(prefers-color-scheme: dark)');
  const escape = value => String(value ?? '').replace(/[&<>"']/g, char => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
  const time = timestamp => timestamp ? new Date(timestamp * 1000).toLocaleTimeString('zh-CN', {hour12:false}) : '—';
  const server = id => data.config.servers.find(item => item.id === id);
  const state = id => {
    const result = data.states[id] || {};
    // 请求受理前先清零显示；新任务的事件到达后，立即使用真实发送进度。
    return startingKnocks.has(id) && startingKnocks.get(id) === result.knock_id
      ? {...result, busy:true, knocking:true, progress:0, knock_state:'queued', knock_message:'等待发送敲门序列'}
      : result;
  };
  const logo = '<svg width="22" height="22" viewBox="0 0 20 20" aria-hidden="true"><rect x="1" y="1" width="18" height="18" rx="5" fill="none" stroke="var(--accent)" stroke-width="1.6"/><circle cx="10" cy="10" r="3" fill="var(--accent)"/></svg>';
  const copyright = '<div class="copyright">Copyright © 2026 Qwerto107. All rights reserved.</div>';
  const logoutIcon = '<svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="M9 4H5.5A1.5 1.5 0 0 0 4 5.5v13A1.5 1.5 0 0 0 5.5 20H9M15 8l4 4-4 4M19 12H9" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  const googleIcon = '<svg width="18" height="18" viewBox="0 0 48 48" aria-hidden="true"><path fill="#EA4335" d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z"/><path fill="#4285F4" d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z"/><path fill="#FBBC05" d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z"/><path fill="#34A853" d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z"/></svg>';

  async function api(path, method = 'GET', body) {
    const response = await fetch(path, {method, headers: {'Content-Type':'application/json','X-Requested-With':'OpenSesame'}, credentials:'same-origin', body: body === undefined ? undefined : JSON.stringify(body)});
    const value = await response.json().catch(() => ({}));
    if (!response.ok) {
      if (response.status === 401 && path.startsWith('/api/')) await initialize();
      if (response.status === 409) { await refresh(); toast('数据已更新，请确认最新内容后重试'); }
      throw new Error(value.error || `请求失败（${response.status}）`);
    }
    return value;
  }
  async function edit(path, value, method = 'PUT', expectedRevision = data.revision) {
    const revision = await api(`/api/${path}`, method, {revision:expectedRevision, value});
    data.revision = revision;
    await refresh();
  }
  function toast(message) {
    const target = document.querySelector('#toast');
    target.textContent = message;
    target.hidden = false;
    clearTimeout(toast.timer);
    toast.timer = setTimeout(() => { target.hidden = true; }, 4000);
  }
  function theme(preference) { applyTheme(preference); }
  media.addEventListener('change', () => theme(settingsDraft?.appearance || data?.config.settings.appearance || initialTheme));

  function statusIcon(kind = 'idle') {
    const path = kind === 'reachable' ? '<path d="M4 7.2l2 2L10 5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>'
      : ['timeout','refused','error'].includes(kind) ? '<path d="M4.5 4.5l5 5M9.5 4.5l-5 5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>'
      : kind === 'checking' ? '' : '<path d="M5.4 5.4a1.6 1.6 0 1 1 2.2 1.5c-.4.2-.6.5-.6.9" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/><circle cx="7" cy="10" r=".7" fill="currentColor"/>';
    return `<svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><circle cx="7" cy="7" r="6" fill="none" stroke="currentColor" stroke-width="1.4" ${kind === 'checking' ? 'stroke-dasharray="4 3"' : ''}/>${path}</svg>`;
  }
  function largeStatusIcon(kind) {
    if (kind !== 'reachable') return statusIcon(kind);
    return '<svg width="40" height="40" viewBox="0 0 40 40" aria-hidden="true"><circle cx="20" cy="20" r="18" fill="var(--green-bg)" stroke="currentColor" stroke-width="1.6"/><path d="M12 20.5l5.5 5.5L28 15" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  }
  function statusLabel(s, result) {
    if (result.status === 'error' && result.knock_state === 'failed' && result.detail === result.knock_message) return '发送失败';
    return ({reachable:s.service_protocol === 'udp' ? '收到响应' : '可连接',refused:'连接被拒绝',timeout:'检测超时',unknown:'无法确认',checking:'检测中',error:'检测失败'})[result.status] || '尚未检测';
  }
  function status(s, result) {
    return `<span class="status ${escape(result.status)}" title="${escape(result.detail)}">${statusIcon(result.status)}${statusLabel(s, result)}</span>`;
  }
  function tag(protocol, port = '') { return `<span class="tag ${protocol}">${protocol.toUpperCase()}${port ? ` ${port}` : ''}</span>`; }
  function toggle(name, checked, label) { return `<button type="button" class="tog ${checked ? 'on' : ''}" role="switch" aria-checked="${checked}" aria-label="${label}" data-toggle="${name}"></button>`; }
  function nav() {
    return `<nav class="nav"><div class="nav-left"><div class="brand">${logo}OpenSesame</div><div class="nav-links"><a class="nl ${page !== 'settings' ? 'on' : ''}" href="#servers">服务器</a><a class="nl ${page === 'settings' ? 'on' : ''}" href="#settings">设置</a></div></div><button class="btn icon" data-action="logout" aria-label="退出登录" title="退出登录">${logoutIcon}</button></nav>`;
  }
  function login() {
    app.innerHTML = `<main class="login"><div class="login-inner"><div class="brand">${logo}OpenSesame</div><h1>登录</h1><div class="subtitle">输入访问密码以继续</div><form class="card login-card" id="login-form"><label class="lbl" for="password">访问密码</label><input id="password" name="password" type="password" class="inp" autocomplete="current-password" required maxlength="1024"><div class="remember"><input id="remember" name="remember" type="checkbox"><label for="remember">在此浏览器保存密码</label></div><div class="error-message" role="alert" hidden></div><button class="btn pri login-submit" type="submit">登录</button>${auth.google ? `<div class="divider">或</div><a class="btn google" href="/auth/google">${googleIcon}使用 Google 登录</a>` : ''}</form></div>${copyright}</main>`;
  }
  const spinner = '<span class="spin" aria-hidden="true"></span>';
  function knockButtonLabel(result) {
    return result.knocking ? `${spinner}开门中` : '开门';
  }
  function miniTrack(s, result) {
    return `<div class="mini-knock" data-mini-knock="${escape(s.id)}" data-knock-id="${result.knock_id}" data-painted-progress="0" aria-label="敲门中，已发送 ${result.progress} / ${s.open_sequence.length} 步"><div class="mini-track">${Array.from({length:Math.min(4,s.open_sequence.length)},(_,index) => `${index ? '<span class="mini-segment"><i></i></span>' : ''}<span class="mini-node" data-step="${s.open_sequence.length === 1 ? 1 : Math.round(index*(s.open_sequence.length-1)/(Math.min(4,s.open_sequence.length)-1))+1}"></span>`).join('')}</div><span>敲门中</span></div>`;
  }
  function list() {
    const checked = data.config.checked_servers;
    return `<main class="content"><div class="page-head"><div><h1>服务器</h1><div class="subtitle">${data.config.settings.auto_check ? `每 ${data.config.settings.check_interval_secs} 秒自动检测端口` : '自动检测已关闭'}</div></div><button class="btn pri add" data-action="add">+ 新增服务器</button></div>
      <div id="batch-live" hidden></div>
      <div class="spread"><label class="selection"><input type="checkbox" class="chk" id="select-all" aria-label="全选服务器" ${!data.config.servers.length ? 'disabled' : ''}><span>全选</span><span class="mono muted">已选 ${checked.length} / ${data.config.servers.length}</span></label><div class="ops"><button class="btn" data-action="batch-edit" ${!checked.length ? 'disabled' : ''}>批量设置</button><button class="btn" data-action="batch-probe" ${!checked.length ? 'disabled' : ''}>批量检测</button><button class="btn pri running-button" data-action="batch-knock" ${!checked.length ? 'disabled' : ''}>批量开门</button></div></div>
      <div class="card table-wrap"><div class="tr heading"><span></span><span>服务器</span><span>业务端口</span><span>端口状态</span><span>最近检测</span><span></span></div>${data.config.servers.map(s => `<div class="tr" data-row="${escape(s.id)}"><input class="chk" type="checkbox" data-select="${escape(s.id)}" aria-label="勾选 ${escape(s.name)}" ${checked.includes(s.id) ? 'checked' : ''}><a class="server-link" href="#server/${encodeURIComponent(s.id)}"><div class="server-name">${escape(s.name)}</div><div class="mono server-host">${escape(s.host)}</div></a><div>${tag(s.service_protocol, s.service_port)}</div><div data-status="${escape(s.id)}">${status(s, state(s.id))}</div><div class="mono last-check" data-time="${escape(s.id)}">${time(state(s.id).checked_at)}</div><div class="row-actions"><button class="btn knock-row running-button" data-knock="${escape(s.id)}">开门</button><button class="btn" data-probe="${escape(s.id)}">检测</button><button class="btn sm" data-menu="${escape(s.id)}" aria-label="更多操作：${escape(s.name)}" aria-expanded="false">⋯</button></div></div>`).join('')}${!data.config.servers.length ? '<div class="empty">还没有服务器，点击“新增服务器”添加。</div>' : ''}</div></main>`;
  }
  function detail() {
    const s = server(currentId);
    if (!s) return '<main class="content"><h1>服务器已删除</h1><a href="#servers">返回服务器</a></main>';
    return `<main class="content detail"><div class="breadcrumbs"><a href="#servers">服务器</a> / ${escape(s.name)}</div><div class="page-head"><div><h1>${escape(s.name)}</h1><div class="mono server-host">${escape(s.host)}</div></div><div class="ops"><button class="btn" data-edit="${escape(s.id)}">编辑</button><button class="btn danger" data-delete="${escape(s.id)}">删除</button></div></div><div id="detail-live">${detailLive(s)}</div></main>`;
  }
  function lockIcon(open) {
    return `<svg width="24" height="24" viewBox="0 0 24 24" aria-hidden="true"><rect x="5" y="11" width="14" height="9" rx="2" fill="none" stroke="currentColor" stroke-width="1.8"/><path d="${open ? 'M8 11V8a4 4 0 0 1 7.5-2' : 'M8 11V8a4 4 0 0 1 8 0v3'}" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>`;
  }
  function knockCard(s, result) {
    const active = result.knocking;
    const hasRun = !!result.knock_id;
    return `<div class="spread"><div><h2>开门序列</h2><div class="knock-message ${active ? 'sending-text' : ''}" data-knock-message>${escape(result.knock_message || `共 ${s.open_sequence.length} 步 · 间隔 ${s.interval_ms} ms`)}</div></div><button class="btn pri knock-button running-button" data-knock="${escape(s.id)}" ${result.busy ? 'disabled' : ''}>${knockButtonLabel(result)}</button></div>
      ${hasRun ? `<div class="path-viewport"><div class="send-path" data-server="${escape(s.id)}" data-painted-progress="${result.progress || 0}"><div class="path-endpoint"><div class="path-source">${logo}</div><div class="endpoint-label">OpenSesame</div></div><div class="path-track"><div class="path-base"></div>${s.open_sequence.slice(1).map((_, index) => `<div class="path-segment" data-segment="${index}"></div>`).join('')}<div class="path-packet" aria-hidden="true"></div>${s.open_sequence.map((step,index) => `<div class="path-node mono" data-step="${index+1}">${index+1}</div><div class="path-label" data-label="${index}"><div class="mono">${step.port}</div>${tag(step.protocol)}</div>`).join('')}</div><div class="path-endpoint"><div class="path-lock" data-phase="${result.knock_state}" aria-label="等待序列发送完成"><span class="lock-closed">${lockIcon(false)}</span><span class="lock-open">${lockIcon(true)}</span></div><div class="endpoint-label">${escape(s.name)}</div></div></div></div>` : `<div class="sequence">${s.open_sequence.map(step => `<span class="chip"><span class="mono">${step.port}</span>${tag(step.protocol)}</span>`).join('<span class="muted">→</span>')}</div>`}
      <div class="send-progress"><div class="shim" data-send-progress></div></div><div class="hint knock-hint" ${active ? 'hidden' : ''}>“已发送”不等于开门成功，请以下方端口检测结果为准。</div>`;
  }
  function portCard(s) {
    const result = state(s.id);
    const settings = data.config.settings;
    const explanation = result.status === 'reachable' ? (s.service_protocol === 'tcp' ? '仅表示连接建立，不代表登录成功' : '仅表示收到响应，不代表登录成功')
      : result.status === 'unknown' || result.status === 'timeout' ? '不代表端口关闭' : result.detail || '等待端口检测';
    const next = autoCheckLabel(s.id);
    return `<div class="spread"><h2>业务端口状态</h2><div class="port-actions"><button class="btn" data-probe="${escape(s.id)}" ${result.busy ? 'disabled' : ''}>检测</button><div class="port-switch">自动检测${toggle('detail-auto', settings.auto_check, '自动检测')}</div></div></div><div class="port-result"><span class="status ${escape(result.status)}">${largeStatusIcon(result.status)}</span><div><div class="port-title status ${escape(result.status)}">${s.service_protocol.toUpperCase()} ${statusLabel(s,result)}</div><div class="port-explanation"><span class="mono">${s.service_port}</span> / ${s.service_protocol.toUpperCase()} · ${escape(explanation)}</div></div></div><div class="stats"><div class="stat"><div class="lbl">最近检测</div><div class="mono">${time(result.checked_at)}</div></div><div class="stat"><div class="lbl">下次检测</div><div class="mono" data-next-check="${escape(s.id)}">${next}</div></div><div class="stat"><div class="lbl">检测周期</div><div class="mono">每 ${settings.check_interval_secs} 秒</div></div></div>`;
  }
  function historyCard(s) {
    return `<h2>最近记录</h2><div class="history">${(state(s.id).history || []).map(record => `<div class="record"><time class="mono">${time(record.at)}</time><span>${escape(record.message)}</span></div>`).join('') || '<div class="hint">暂无记录</div>'}</div>`;
  }
  function detailLive(s) {
    return `<div class="detail-columns"><div class="stack detail-main"><section class="card" id="knock-card" data-knock-id="${state(s.id).knock_id || 0}">${knockCard(s,state(s.id))}</section><section class="card" id="port-card">${portCard(s)}</section></div><aside class="stack detail-side"><section class="card" id="history-card">${historyCard(s)}</section></aside></div>`;
  }
  function segment(key, options, selected) {
    return options.map(([value,label]) => `<button type="button" class="${value === selected ? 'on' : ''}" data-setting="${key}" data-value="${value}" aria-pressed="${value === selected}">${label}</button>`).join('');
  }
  function settings() {
    const s = settingsDraft;
    return `<main class="content settings"><h1>设置</h1><section class="card"><h2>端口检测</h2><div class="setting-row"><div><div class="setting-title">自动检测端口状态</div><div class="setting-hint">网页可见且已登录时按间隔检测，切到后台或关闭网页后暂停</div></div>${toggle('auto_check', s.auto_check, '自动检测端口状态')}</div><div class="scope"><div class="lbl">检测范围</div><div class="seg">${segment('check_all', [['true','全部服务器'],['false','仅当前服务器']], String(s.check_all))}</div></div><div class="fields"><div class="field"><label class="lbl" for="interval">检测间隔（秒）</label><input id="interval" class="inp mono" type="number" min="1" max="86400" value="${s.check_interval_secs}" data-number="check_interval_secs"></div><div class="field"><label class="lbl" for="timeout">单次超时（秒）</label><input id="timeout" class="inp mono" type="number" min="1" max="120" value="${s.check_timeout_secs}" data-number="check_timeout_secs"></div></div></section><section class="card"><h2>外观</h2><div class="seg appearance">${segment('appearance', [['system','跟随系统'],['dark','深色'],['light','亮色']], s.appearance)}</div></section><section class="card"><h2>登录</h2><div class="setting-row"><div class="saved-login">${auth.remembered ? '此浏览器已保存密码' : '此浏览器未保存密码'}</div><div class="ops"><button class="btn" data-action="forget" ${!auth.remembered ? 'disabled' : ''}>清除保存的密码</button><button class="btn danger" data-action="logout">退出登录</button></div></div></section><div class="settings-actions"><button class="btn" data-action="cancel-settings">取消</button><button class="btn pri" data-action="save-settings">保存</button></div></main>`;
  }
  function render() {
    closeMenu();
    app.removeAttribute('aria-busy');
    if (!auth?.authenticated) { login(); return; }
    app.innerHTML = `<div class="shell">${nav()}${page === 'settings' ? settings() : page === 'detail' ? detail() : list()}</div>${copyright}`;
    const all = document.querySelector('#select-all');
    if (all) { all.checked = data.config.servers.length > 0 && data.config.checked_servers.length === data.config.servers.length; all.indeterminate = data.config.checked_servers.length > 0 && !all.checked; }
    progressBars();
    paintRuntime();
  }
  function progressBars() { document.querySelectorAll('[data-progress]').forEach(el => { el.style.width = `${Number(el.dataset.progress) || 0}%`; }); }
  function mergeStates(instance, states) {
    if (data.instance !== instance) { data.instance = instance; data.states = {}; }
    for (const [id, next] of Object.entries(states)) {
      if (!data.states[id] || next.version >= data.states[id].version) data.states[id] = next;
    }
  }
  function closeProgress() {
    progressSource?.close();
    progressSource = null;
    progressReady = null;
  }
  function watchProgress() {
    if (progressSource) return progressReady;
    if (!window.EventSource || document.hidden || !auth?.authenticated) return Promise.resolve();
    const source = new EventSource('/api/events');
    progressSource = source;
    progressReady = new Promise(resolve => {
      const timer = setTimeout(resolve, 1500);
      const ready = () => { clearTimeout(timer); resolve(); };
      source.onopen = ready;
      source.onerror = ready;
    });
    const accept = (event, snapshot) => {
      if (progressSource !== source || !data) return;
      const update = JSON.parse(event.data);
      mergeStates(update.instance, snapshot ? update.states : {[update.id]:update.state});
      paintRuntime();
      schedulePoll();
      syncProgressStream();
    };
    source.addEventListener('snapshot', event => accept(event,true));
    source.addEventListener('server', event => accept(event,false));
    return progressReady;
  }
  function syncProgressStream() {
    if (document.hidden || !auth?.authenticated || (!pendingKnocks && !Object.values(data?.states || {}).some(result => result.knocking))) closeProgress();
    else watchProgress();
  }
  async function startKnock(ids) {
    ids.forEach(id => { startingKnocks.set(id,state(id).knock_id); lastAutoCheck.set(id,Date.now()); });
    pendingKnocks++;
    paintRuntime();
    try {
      // 先订阅再发起任务，默认 200 ms 的步骤也不会被一秒轮询漏过。
      await watchProgress();
      await api('/api/knock','POST',{ids});
      await refresh();
      paintRuntime();
    } finally {
      ids.forEach(id => startingKnocks.delete(id));
      pendingKnocks--;
      paintRuntime();
      syncProgressStream();
    }
  }
  function pulseNode(node, small) {
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    node.animate([
      {boxShadow:'0 0 0 0 var(--pulse-strong)'},
      {boxShadow:`0 0 0 ${small ? 7 : 16}px var(--pulse-clear)`},
    ], {duration:840,easing:'ease-out'});
  }
  function paintTrack(track, result, small) {
    const previous = Number(track.dataset.paintedProgress);
    const nodes = [...track.querySelectorAll(small ? '.mini-node' : '.path-node')];
    const segments = [...track.querySelectorAll(small ? '.mini-segment i' : '.path-segment')];
    const total = nodes.length;
    nodes.forEach((node,index) => {
      node.classList.toggle('done', Number(node.dataset.step) <= result.progress);
      if (Number(node.dataset.step) > previous && Number(node.dataset.step) <= result.progress) pulseNode(node,small);
      if (!small) node.style.left = `${total === 1 ? 50 : index/(total-1)*100}%`;
    });
    segments.forEach((segment,index) => {
      segment.classList.toggle('done', Number(nodes[index+1].dataset.step) <= result.progress);
      if (!small) { segment.style.left = `${index/(total-1)*100}%`; segment.style.width = `${100/(total-1)}%`; }
    });
    if (!small) {
      track.querySelector('.path-track').style.minWidth = `${Math.max(260,(total-1)*86.667)}px`;
      track.querySelectorAll('[data-label]').forEach((label,index) => { label.style.left = `${total === 1 ? 50 : index/(total-1)*100}%`; });
      const packet = track.querySelector('.path-packet');
      packet.style.left = `${total === 1 ? 50 : Math.max(0,result.progress-1)/(total-1)*100}%`;
      packet.classList.toggle('visible', result.progress > 0 && result.knocking);
      const lock = track.querySelector('.path-lock');
      const sent = result.knock_state === 'sent';
      lock.classList.toggle('sent',sent);
      lock.setAttribute('aria-label',sent ? '敲门序列已发送，端口状态另行检测' : '等待序列发送完成');
      if (sent && lock.dataset.phase !== 'sent' && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
        lock.animate([{boxShadow:'0 0 0 0 var(--success-pulse)'},{boxShadow:'0 0 0 30px var(--pulse-clear)'}],{duration:1000,easing:'ease-out'});
      }
      lock.dataset.phase = result.knock_state;
    }
    track.dataset.paintedProgress = result.progress || 0;
  }
  function paintBatch() {
    const host = document.querySelector('#batch-live');
    if (!host) return;
    if (!batchIds.length) {
      const active = data.config.checked_servers.filter(id => state(id).knocking);
      if (active.length > 1) batchIds = [...data.config.checked_servers];
    }
    const items = batchIds.map(server).filter(Boolean);
    host.hidden = !items.length;
    if (host.hidden) return;
    if (!host.firstElementChild) host.innerHTML = `<section class="card batch-progress-card"><div class="spread"><div class="batch-progress-title"><span class="spin" aria-hidden="true"></span><span data-batch-title></span></div><span class="mono muted" data-batch-count></span></div><div class="send-progress"><div class="shim"></div></div></section>`;
    const running = items.some(s => state(s.id).knocking);
    const checking = items.some(s => state(s.id).busy);
    const sent = items.filter(s => state(s.id).knock_state === 'sent' && state(s.id).knock_id !== batchPreviousRuns.get(s.id)).length;
    const failed = items.filter(s => state(s.id).knock_state === 'failed' && state(s.id).knock_id !== batchPreviousRuns.get(s.id)).length;
    host.querySelector('[data-batch-title]').textContent = running || pendingKnocks ? '批量开门中' : checking ? '正在检测端口' : failed ? '批量开门结束' : '批量开门完成';
    host.querySelector('[data-batch-count]').textContent = `${sent} / ${items.length} 台已发送${failed ? ` · ${failed} 台发送失败` : ''}`;
    host.querySelector('.spin').hidden = !running && !checking && !pendingKnocks;
    const totalSteps = items.reduce((total,s) => total+s.open_sequence.length,0);
    const sentSteps = items.reduce((total,s) => total+(state(s.id).knock_id === batchPreviousRuns.get(s.id) ? 0 : state(s.id).progress || 0),0);
    host.querySelector('.shim').style.transition = items.every(s => state(s.id).knock_state === 'queued') ? 'none' : '';
    host.querySelector('.shim').style.width = `${sentSteps/totalSteps*100}%`;
    host.querySelector('.shim').classList.toggle('moving',running || checking);
  }
  function paintRuntime() {
    if (!data || !auth?.authenticated) return;
    if (page === 'servers') {
      paintBatch();
      document.querySelectorAll('[data-row]').forEach(row => {
        const s = server(row.dataset.row);
        if (!s) return;
        const result = state(s.id);
        const cell = row.querySelector('[data-status]');
        const key = result.knocking ? `knock:${result.knock_id}` : `${result.status}:${result.detail}`;
        if (cell.dataset.view !== key) {
          cell.innerHTML = result.knocking ? miniTrack(s,result) : status(s,result);
          cell.dataset.view = key;
        }
        const track = cell.querySelector('.mini-knock');
        if (track) { paintTrack(track,result,true); track.setAttribute('aria-label',`敲门中，已发送 ${result.progress} / ${s.open_sequence.length} 步`); }
        row.querySelector('[data-time]').textContent = time(result.checked_at);
        const knock = row.querySelector('[data-knock]');
        knock.disabled = !!result.busy;
        const label = knockButtonLabel(result);
        if (knock.innerHTML !== label) knock.innerHTML = label;
        row.querySelector('[data-probe]').disabled = !!result.busy;
      });
      const ids = data.config.checked_servers;
      const busy = ids.some(id => state(id).busy);
      const running = batchIds.some(id => state(id).knocking);
      const button = document.querySelector('[data-action="batch-knock"]');
      if (button) {
        button.disabled = !ids.length || busy || pendingKnocks > 0;
        const label = running ? `${spinner}批量开门中` : '批量开门';
        if (button.innerHTML !== label) button.innerHTML = label;
      }
      const probe = document.querySelector('[data-action="batch-probe"]');
      if (probe) probe.disabled = !ids.length || busy;
    } else if (page === 'detail') {
      const s = server(currentId);
      if (!s) return;
      const result = state(s.id);
      const card = document.querySelector('#knock-card');
      if (!card) return;
      if (card.dataset.knockId !== String(result.knock_id || 0)) {
        card.innerHTML = knockCard(s,result);
        card.dataset.knockId = result.knock_id || 0;
      }
      card.querySelector('[data-knock-message]').textContent = result.knocking && result.knock_state !== 'queued' ? `正在发送第 ${Math.min(result.progress+1,s.open_sequence.length)} / ${s.open_sequence.length} 步…` : (result.knock_message || `共 ${s.open_sequence.length} 步 · 间隔 ${s.interval_ms} ms`);
      card.querySelector('[data-knock-message]').classList.toggle('sending-text',!!result.knocking);
      const button = card.querySelector('[data-knock]');
      button.disabled = !!result.busy;
      const label = knockButtonLabel(result);
      if (button.innerHTML !== label) button.innerHTML = label;
      const progress = card.querySelector('[data-send-progress]');
      progress.style.transition = result.knock_state === 'queued' ? 'none' : '';
      progress.style.width = `${(result.progress || 0)/s.open_sequence.length*100}%`;
      progress.classList.toggle('moving',!!result.knocking);
      card.querySelector('.knock-hint').hidden = !!result.knocking;
      const path = card.querySelector('.send-path');
      if (path) paintTrack(path,result,false);
      const port = document.querySelector('#port-card');
      const portKey = `${result.status}:${result.detail}:${result.busy}:${result.checked_at}:${data.config.settings.auto_check}`;
      if (port.dataset.view !== portKey && !port.contains(document.activeElement)) { port.innerHTML = portCard(s); port.dataset.view = portKey; }
      const history = document.querySelector('#history-card');
      const historyKey = JSON.stringify(result.history || []);
      if (history.dataset.view !== historyKey) { history.innerHTML = historyCard(s); history.dataset.view = historyKey; }
    }
  }
  async function refresh() {
    lastSync = Date.now();
    const next = await api('/api/state');
    const previous = data;
    if (!data || next.revision >= data.revision) {
      data = next;
      if (previous?.instance === next.instance) mergeStates(previous.instance,previous.states);
    }
    initialTheme = data.config.settings.appearance;
    try { localStorage.setItem(themeKey, initialTheme); } catch {}
    syncProgressStream();
    schedulePoll();
  }
  async function initialize() {
    auth = await api('/auth/status');
    if (!auth.authenticated) { closeProgress(); clearTimeout(pollTimer); lastAutoCheck.clear(); data = null; if (dialog.open) dialog.close(); theme(initialTheme); render(); return; }
    await refresh();
    await route();
  }
  async function route() {
    if (!auth?.authenticated) return;
    const hash = location.hash.slice(1);
    settingsDraft = null;
    if (hash === 'settings') { page = 'settings'; settingsDraft = structuredClone(data.config.settings); draftRevision = data.revision; }
    else if (hash.startsWith('server/')) {
      currentId = decodeURIComponent(hash.slice(7)); page = 'detail';
      if (server(currentId) && data.config.current_server !== currentId) await edit('current', currentId);
    } else page = 'servers';
    theme(settingsDraft?.appearance || data.config.settings.appearance);
    render();
  }
  window.addEventListener('hashchange', () => route().catch(error => toast(error.message)));

  function showModal(content) {
    modalRevision = data.revision;
    dialog.innerHTML = content;
    dialog.showModal();
  }
  const field = (name,label,value,type = 'text',attrs = '') => `<div><label class="lbl" for="${name}">${label}</label><input class="inp" id="${name}" name="${name}" type="${type}" value="${escape(value)}" ${attrs}></div>`;
  function stepRow(step, index) {
    return `<div class="step-row"><span class="ix">${index+1}</span><input class="inp mono" type="number" min="1" max="65535" value="${step.port}" aria-label="第 ${index+1} 步端口" required><select class="inp" aria-label="第 ${index+1} 步协议"><option value="tcp" ${step.protocol === 'tcp' ? 'selected' : ''}>TCP</option><option value="udp" ${step.protocol === 'udp' ? 'selected' : ''}>UDP</option></select><button class="btn sm" type="button" data-action="remove-step" aria-label="删除第 ${index+1} 步">×</button></div>`;
  }
  function editor(id) {
    const s = id ? server(id) : {name:'',host:'',open_sequence:[{port:7000,protocol:'tcp'}],interval_ms:200,sequence_timeout_secs:10,service_port:22,service_protocol:'tcp',check_interval_secs:30,check_timeout_secs:3};
    showModal(`<form id="server-form" data-id="${id || ''}"><div class="modal-head"><h2>${id ? '编辑服务器' : '新增服务器'}</h2></div><div class="modal-grid">${field('name','名称',s.name,'text','required maxlength="256"')}${field('host','IP 地址或主机名',s.host,'text','required maxlength="253"')}${field('service_port','业务端口',s.service_port,'number','required min="1" max="65535"')}<div><label class="lbl" for="service_protocol">业务协议</label><select class="inp" id="service_protocol" name="service_protocol"><option value="tcp" ${s.service_protocol === 'tcp' ? 'selected' : ''}>TCP</option><option value="udp" ${s.service_protocol === 'udp' ? 'selected' : ''}>UDP</option></select></div>${field('interval_ms','步骤间隔（毫秒）',s.interval_ms,'number','required min="10" max="60000"')}${field('sequence_timeout_secs','序列超时（秒）',s.sequence_timeout_secs,'number','required min="1" max="300"')}</div><section class="modal-section"><h2>开门序列</h2><div id="steps">${s.open_sequence.map(stepRow).join('')}</div><button class="btn modal-section" type="button" data-action="add-step">+ 添加步骤</button></section><div class="error-message" role="alert" hidden></div><div class="modal-footer"><button class="btn" type="button" data-action="close-modal">取消</button><button class="btn pri" type="submit">保存</button></div></form>`);
  }
  function readSteps() { return [...dialog.querySelectorAll('.step-row')].map(row => ({port:Number(row.querySelector('input').value),protocol:row.querySelector('select').value})); }
  function batchEditor() {
    const s = server(data.config.checked_servers[0]);
    showModal(`<form id="batch-form"><div class="modal-head"><h2>批量设置 · ${data.config.checked_servers.length} 台</h2></div><label class="selection"><input type="checkbox" class="chk" name="change_service">修改业务端口和协议</label><div class="fields"><div class="field">${field('service_port','业务端口',s.service_port,'number','required min="1" max="65535"')}</div><div class="field"><label class="lbl" for="service_protocol">业务协议</label><select class="inp" id="service_protocol" name="service_protocol"><option value="tcp">TCP</option><option value="udp">UDP</option></select></div></div><section class="modal-section"><label class="selection"><input type="checkbox" class="chk" name="change_sequence">修改开门序列</label><div id="steps">${s.open_sequence.map(stepRow).join('')}</div><button class="btn modal-section" type="button" data-action="add-step">+ 添加步骤</button><div class="hint">仅更新勾选的项目，保留每台服务器的其他设置。</div></section><div class="error-message" role="alert" hidden></div><div class="modal-footer"><button class="btn" type="button" data-action="close-modal">取消</button><button class="btn pri" type="submit">保存</button></div></form>`);
  }
  function confirmDelete(id) {
    showModal(`<form id="delete-form" data-id="${escape(id)}"><div class="modal-head"><h2>删除服务器</h2></div><p>确定删除“${escape(server(id).name)}”吗？</p><div class="error-message" role="alert" hidden></div><div class="modal-footer"><button type="button" class="btn" data-action="close-modal">取消</button><button type="submit" class="btn danger">删除</button></div></form>`);
  }
  dialog.addEventListener('close', () => { dialog.innerHTML = ''; });

  document.addEventListener('submit', async event => {
    const form = event.target;
    if (!(form instanceof HTMLFormElement)) return;
    event.preventDefault();
    const button = form.querySelector('[type=submit]');
    button.disabled = true;
    const values = Object.fromEntries(new FormData(form));
    try {
      if (form.id === 'login-form') {
        await api('/auth/login','POST',{password:values.password,remember:values.remember === 'on'});
        form.reset(); await initialize();
      } else if (form.id === 'server-form') {
        const id = form.dataset.id;
        const value = {...(id ? server(id) : {}),...values,open_sequence:readSteps()};
        for (const key of ['service_port','interval_ms','sequence_timeout_secs']) value[key] = Number(value[key]);
        await edit(id ? `servers/${encodeURIComponent(id)}` : 'servers', value, id ? 'PUT' : 'POST', modalRevision);
        dialog.close(); render(); toast('服务器已保存');
      } else if (form.id === 'batch-form') {
        if (!values.change_service && !values.change_sequence) throw new Error('请选择需要修改的项目');
        await edit('batch',{ids:data.config.checked_servers,sequence:values.change_sequence ? readSteps() : null,service:values.change_service ? [Number(values.service_port),values.service_protocol] : null}, 'PUT', modalRevision);
        dialog.close(); render(); toast('批量设置已保存');
      } else if (form.id === 'delete-form') {
        await edit(`servers/${encodeURIComponent(form.dataset.id)}`,null,'DELETE',modalRevision);
        dialog.close(); location.hash = 'servers'; render(); toast('服务器已删除');
      }
    } catch (error) {
      const target = form.querySelector('.error-message');
      target.textContent = error.message; target.hidden = false;
    } finally { button.disabled = false; }
  });

  document.addEventListener('change', async event => {
    const target = event.target;
    try {
      if (target.id === 'select-all' || target.dataset.select) {
        const ids = new Set(data.config.checked_servers);
        if (target.id === 'select-all') { ids.clear(); if (target.checked) data.config.servers.forEach(s => ids.add(s.id)); }
        else if (target.checked) ids.add(target.dataset.select); else ids.delete(target.dataset.select);
        await edit('selection', [...ids]); render();
      }
    } catch (error) { toast(error.message); render(); }
  });
  document.addEventListener('input', event => {
    if (event.target.dataset.number && settingsDraft) settingsDraft[event.target.dataset.number] = Number(event.target.value);
  });

  function closeMenu() {
    document.querySelector('.menu')?.remove();
    menuAnchor?.setAttribute('aria-expanded', 'false');
    menuAnchor = null;
  }
  function openMenu(anchor) {
    const wasOpen = menuAnchor === anchor;
    closeMenu();
    if (wasOpen) return;
    const id = escape(anchor.dataset.menu);
    const busy = state(anchor.dataset.menu).busy;
    // 挂到页面根部，避免表格的 overflow 裁切菜单或被菜单撑出滚动条。
    document.body.insertAdjacentHTML('beforeend', `<div class="menu"><button data-edit="${id}">编辑</button><button data-probe="${id}" ${busy ? 'disabled' : ''}>立即检测</button><button class="danger" data-delete="${id}">删除</button></div>`);
    const menu = document.querySelector('.menu');
    const rect = anchor.getBoundingClientRect();
    const size = menu.getBoundingClientRect();
    const edge = 8;
    const below = rect.bottom + 4;
    const top = below + size.height <= window.innerHeight - edge ? below : rect.top - size.height - 4;
    menu.style.left = `${Math.max(edge, Math.min(rect.right - size.width, window.innerWidth - size.width - edge))}px`;
    menu.style.top = `${Math.max(edge, top)}px`;
    menuAnchor = anchor;
    anchor.setAttribute('aria-expanded', 'true');
  }
  document.addEventListener('scroll', closeMenu, true);
  window.addEventListener('resize', closeMenu);
  document.addEventListener('keydown', event => {
    if (event.key === 'Escape' && menuAnchor) {
      const anchor = menuAnchor;
      closeMenu();
      anchor.focus();
    }
  });

  document.addEventListener('click', async event => {
    const target = event.target.closest('button, [data-action]');
    if (!target || target.disabled) return;
    try {
      if (target.dataset.knock) {
        target.disabled = true;
        await startKnock([target.dataset.knock]);
      } else if (target.dataset.edit) editor(target.dataset.edit);
      else if (target.dataset.delete) confirmDelete(target.dataset.delete);
      else if (target.dataset.probe) {
        target.disabled = true;
        await api('/api/probe','POST',{ids:[target.dataset.probe]});
        // 与开门操作一致，立即读取 busy 状态并切换到任务中的一秒同步。
        await refresh();
        render();
        toast('已开始检测');
      }
      else if (target.dataset.menu) {
        openMenu(target);
      } else if (target.dataset.toggle) {
        if (target.dataset.toggle === 'detail-auto') {
          await edit('settings',{...data.config.settings,auto_check:!data.config.settings.auto_check}); render();
        } else { settingsDraft[target.dataset.toggle] = !settingsDraft[target.dataset.toggle]; render(); }
      } else if (target.dataset.setting) {
        settingsDraft[target.dataset.setting] = target.dataset.setting === 'check_all' ? target.dataset.value === 'true' : target.dataset.value;
        theme(settingsDraft.appearance); render();
      } else {
        switch (target.dataset.action) {
          case 'add': editor(); break;
          case 'batch-edit': batchEditor(); break;
          case 'batch-knock':
            batchIds = [...data.config.checked_servers];
            batchPreviousRuns = new Map(batchIds.map(id => [id,state(id).knock_id]));
            await startKnock(batchIds); break;
          case 'batch-probe':
            await api('/api/probe','POST',{ids:data.config.checked_servers}); await refresh(); paintRuntime(); toast('已开始批量检测'); break;
          case 'close-modal': dialog.close(); break;
          case 'add-step': {
            const steps = readSteps();
            if (steps.length >= 256) throw new Error('最多支持 256 个步骤');
            steps.push({port:7000,protocol:'tcp'}); dialog.querySelector('#steps').innerHTML = steps.map(stepRow).join(''); break;
          }
          case 'remove-step': {
            const index = [...dialog.querySelectorAll('.step-row')].indexOf(target.closest('.step-row'));
            const steps = readSteps();
            if (steps.length === 1) throw new Error('至少保留一个敲门步骤');
            steps.splice(index,1); dialog.querySelector('#steps').innerHTML = steps.map(stepRow).join(''); break;
          }
          case 'save-settings':
            await edit('settings', settingsDraft, 'PUT', draftRevision); settingsDraft = structuredClone(data.config.settings); draftRevision = data.revision; render(); toast('设置已保存'); break;
          case 'cancel-settings': settingsDraft = structuredClone(data.config.settings); draftRevision = data.revision; theme(settingsDraft.appearance); render(); break;
          case 'logout': await api('/api/logout','POST',{}); await initialize(); break;
          case 'forget': await api('/api/forget','POST',{}); auth = await api('/auth/status'); render(); toast('已清除保存的登录状态'); break;
        }
      }
    } catch (error) { toast(error.message); target.disabled = false; }
    if (!target.dataset.menu) closeMenu();
  });
  document.addEventListener('click', event => {
    if (!event.target.closest('[data-menu], .menu')) closeMenu();
  });
  function nextAutoCheck(id) {
    return Math.max(lastAutoCheck.get(id) || 0, (state(id).checked_at || 0)*1000)
      + data.config.settings.check_interval_secs*1000;
  }
  function autoCheckLabel(id) {
    const settings = data.config.settings;
    if (!auth?.authenticated || document.hidden || !settings.auto_check || (!settings.check_all && data.config.current_server !== id)) return '已暂停';
    if (state(id).busy) return '任务结束后';
    const remaining = Math.ceil((nextAutoCheck(id)-Date.now())/1000);
    return remaining > 0 ? `${remaining} 秒后` : '即将检测';
  }
  async function autoCheck() {
    if (!auth?.authenticated || !data || document.hidden || !data.config.settings.auto_check || autoChecking || polling) return;
    const now = Date.now();
    const settings = data.config.settings;
    const ids = data.config.servers.filter(s => {
      // 忙碌期间推迟下一次周期，避免敲门结束或发送失败后立即再检测。
      if (state(s.id).busy) { lastAutoCheck.set(s.id,now); return false; }
      return (settings.check_all || data.config.current_server === s.id) && nextAutoCheck(s.id) <= now;
    }).map(s => s.id);
    if (!ids.length) return;
    autoChecking = true;
    ids.forEach(id => lastAutoCheck.set(id,now));
    try {
      await api('/api/probe','POST',{ids});
      if (!auth?.authenticated) return;
      await refresh();
      paintRuntime();
    } catch (error) { if (auth?.authenticated) toast(error.message); }
    finally { autoChecking = false; }
  }
  function schedulePoll() {
    clearTimeout(pollTimer);
    if (!auth?.authenticated || document.hidden || polling) return;
    const interval = Object.values(data.states).some(result => result.busy) ? 1000 : 10000;
    pollTimer = setTimeout(syncState, Math.max(0, interval - (Date.now() - lastSync)));
  }
  document.addEventListener('visibilitychange', () => {
    if (!document.hidden) lastSync = 0;
    syncProgressStream();
    schedulePoll();
  });
  setInterval(() => {
    if (document.hidden || !auth?.authenticated || !data) return;
    autoCheck();
    document.querySelectorAll('[data-next-check]').forEach(el => { el.textContent = autoCheckLabel(el.dataset.nextCheck); });
  }, 1000);
  async function syncState() {
    if (!auth?.authenticated || polling || document.hidden) return;
    // 按上次请求时间安排下一次同步，避免固定定时器的时差让一秒变成两秒。
    polling = true;
    try {
      const revision = data.revision;
      await refresh();
      if (dialog.open) return;
      if (page === 'settings') return;
      if (revision !== data.revision) { render(); return; }
      paintRuntime();
      progressBars();
    } catch (error) { toast(error.message); }
    finally { polling = false; schedulePoll(); }
  }
  initialize().catch(error => { app.innerHTML = `<main class="content"><h1>连接失败</h1><div class="error-summary">${escape(error.message)}</div><button class="btn" data-action="reload">重新连接</button></main>`; });
  document.addEventListener('click', event => { if (event.target.dataset.action === 'reload') location.reload(); });
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', startApp, {once:true});
} else {
  startApp();
}
