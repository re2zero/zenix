/* Shared primitives and the shell chrome for the zenix interface redesign boards. */

const ICONS = {
  grid: '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>',
  users: '<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>',
  gauge: '<path d="M12 14l4-4"/><path d="M3.34 19a10 10 0 1 1 17.32 0"/><circle cx="12" cy="14" r="1.2"/>',
  pulse: '<path d="M3 12h4l2.5-7 4 14L16 12h5"/>',
  plug: '<path d="M12 22v-5"/><path d="M9 7V2"/><path d="M15 7V2"/><path d="M6 7h12v4a6 6 0 0 1-12 0z"/>',
  gear: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 1.55V21a2 2 0 1 1-4 0v-.09A1.7 1.7 0 0 0 9 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-1.55-1H3a2 2 0 1 1 0-4h.09A1.7 1.7 0 0 0 4.6 9a1.7 1.7 0 0 0-.34-1.88l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-1.55V3a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1 1.55 1.7 1.7 0 0 0 1.88-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.7 1.7 0 0 0 19.4 9a1.7 1.7 0 0 0 1.55 1H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.51 1z"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="M21 21l-4.3-4.3"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  split: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/>',
  x: '<path d="M18 6L6 18M6 6l12 12"/>',
  'chevron-r': '<path d="M9 18l6-6-6-6"/>',
  'chevron-d': '<path d="M6 9l6 6 6-6"/>',
  'chevron-u': '<path d="M18 15l-6-6-6 6"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  layout: '<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18M9 21V9"/>',
  zap: '<path d="M13 2L3 14h8l-1 8 11-13h-8z"/>',
  branch: '<path d="M6 3v12"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/>',
  terminal: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3"/><path d="M13 15h4"/>',
  chat: '<path d="M21 15a2 2 0 0 1-2 2H8l-4 4V5a2 2 0 0 1 2-2h13a2 2 0 0 1 2 2z"/><path d="M8 9h8M8 13h5"/>',
  panel: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/>',
  copy: '<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-2.64-6.36"/><path d="M21 3v6h-6"/>',
  check: '<path d="M20 6L9 17l-5-5"/>',
  checkc: '<circle cx="12" cy="12" r="9"/><path d="M8.5 12.5l2.2 2.2 4.8-5"/>',
  alert: '<path d="M10.3 3.9L1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"/><path d="M12 9v4M12 17h.01"/>',
  trash: '<path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M19 6l-1 14H6L5 6"/><path d="M10 11v5M14 11v5"/>',
  edit: '<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>',
  'arrow-up': '<path d="M12 19V5M5 12l7-7 7 7"/>',
  'arrow-up-r': '<path d="M7 17L17 7M7 7h10v10"/>',
  play: '<path d="M6 4l14 8-14 8z"/>',
  pause: '<path d="M7 4h4v16H7zM13 4h4v16h-4z"/>',
  more: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
  external: '<path d="M15 3h6v6"/><path d="M10 14L21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
  minus: '<path d="M5 12h14"/>',
  square: '<rect x="5" y="5" width="14" height="14" rx="1.5"/>',
  maximize: '<rect x="4" y="4" width="16" height="16" rx="2"/>',
  file: '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/><path d="M8 13h8M8 17h5"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  shield: '<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="M9 12l2 2 4-4"/>',
  link: '<path d="M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1.5 1.5"/><path d="M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1.5-1.5"/>',
  box: '<path d="M21 8l-9-5-9 5 9 5 9-5z"/><path d="M3 8v8l9 5 9-5V8"/><path d="M12 13v8"/>',
  command: '<path d="M9 6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3z"/>',
  list: '<path d="M8 6h13M8 12h13M8 18h13"/><circle cx="3.5" cy="6" r="1"/><circle cx="3.5" cy="12" r="1"/><circle cx="3.5" cy="18" r="1"/>',
  eye: '<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
  sliders: '<path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3"/><path d="M1 14h6M9 8h6M17 16h6"/>',
  download: '<path d="M12 3v12M7 10l5 5 5-5"/><path d="M5 21h14"/>',
  upload: '<path d="M12 21V9M7 14l5-5 5 5"/><path d="M5 3h14"/>',
  circle: '<circle cx="12" cy="12" r="8"/>',
  spark: '<path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z"/><path d="M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>',
  lock: '<rect x="4" y="10" width="16" height="11" rx="2"/><path d="M8 10V7a4 4 0 0 1 8 0v3"/>',
  package: '<path d="M16 3H8L4 9v11a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1V9z"/><path d="M4 9h16"/><path d="M9 13h6"/>',
  history: '<path d="M3 12a9 9 0 1 0 3-6.7"/><path d="M3 4v5h5"/><path d="M12 7v5l3 2"/>',
  filter: '<path d="M3 5h18l-7 8v6l-4 2v-8z"/>',
};

function icon(name, cls = '') {
  return `<svg class="ic ${cls}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name] || ICONS.circle}</svg>`;
}

const kbd = (text) => `<span class="kbd">${text}</span>`;
const badge = (text, tone = '') => `<span class="badge ${tone}">${text}</span>`;
const dot = (tone = '') => `<span class="dot ${tone}"></span>`;
const pin = (n, x, y, tip) => `<div class="pin" style="left:${x}px;top:${y}px" data-tip="${tip}">${n}</div>`;
const esc = (text) => String(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/* ---------- window chrome ---------- */

function titlebar(o = {}) {
  const offline = o.offline;
  return `<div class="titlebar">
    <div class="brand"><div class="brand-mark">z</div><div class="brand-name">zenix</div></div>
    <div class="ws-pill">
      ${dot(offline ? 'err' : 'ok')}
      <span>${o.workspace || 'term-ide'}</span>
      ${o.noWorkspace ? '' : `<span class="branch">${icon('branch', 'xs')} main</span>`}
    </div>
    <div class="titlebar-center">
      <div class="search-trigger">${icon('search', 'sm')}<span>${o.searchHint || '搜索工作区、命令或插件 action…'}</span><span style="margin-left:auto;display:flex;gap:3px">${kbd('⌘')}${kbd('⇧')}${kbd('P')}</span></div>
    </div>
    <div class="titlebar-right">
      <button class="iconbtn" title="对话">${icon('chat')}</button>
      <button class="iconbtn" title="命令面板">${icon('command')}</button>
      <div class="win-controls">
        <button class="iconbtn">${icon('minus', 'sm')}</button>
        <button class="iconbtn">${icon('square', 'sm')}</button>
        <button class="iconbtn close">${icon('x', 'sm')}</button>
      </div>
    </div>
  </div>`;
}

function rail(active = 'workspaces') {
  const item = (id, ico, label, badgeText) => `<div class="rail-btn ${active === id ? 'on' : ''}" title="${label}">${icon(ico)}${badgeText ? `<span class="rail-badge">${badgeText}</span>` : ''}</div>`;
  return `<div class="rail">
    ${item('workspaces', 'grid', '工作区')}
    ${item('agents', 'users', '智能体', '3')}
    ${item('fleet', 'gauge', '舰队')}
    ${item('events', 'pulse', '事件')}
    ${item('plugins', 'plug', '插件')}
    <div class="rail-sep"></div>
    <div class="rail-bottom">
      <div class="rail-btn" title="设置">${icon('gear')}</div>
      <div class="rail-btn" title="帮助">${icon('info')}</div>
    </div>
  </div>`;
}

function statusbar(o = {}) {
  if (o.offline) {
    return `<div class="statusbar">
      <div class="group">${dot('err')}<span>未连接</span><span class="dim">·</span><span>没有工作区</span></div>
      <div class="right">
        <span class="seg">${icon('alert', 'xs')} daemon 未启动</span>
        <span class="seg"><span class="kbd">ctrl</span><span class="kbd">b</span> 前缀</span>
        <span class="seg dim">snapshot 等待中</span>
      </div>
    </div>`;
  }
  return `<div class="statusbar">
    <div class="group">${dot('ok')}<span>已连接</span><span class="dim">·</span><span>${o.workspace || 'term-ide'}</span><span class="dim">·</span><span>10 工作区 · 10 标签 · 33 窗格</span></div>
    <div class="right">
      <span class="seg">${icon('branch', 'xs')} main</span>
      <span class="seg warn">${icon('alert', 'xs')} 2</span>
      <span class="seg"><span class="kbd">ctrl</span><span class="kbd">b</span> 前缀</span>
      <span class="seg dim">snapshot</span>
    </div>
  </div>`;
}

function asideRail(active = '') {
  const b = (id, ico, label, extra = '') => `<button class="iconbtn ${active === id ? 'on' : ''}" title="${label}">${icon(ico)}${extra}</button>`;
  return `<div class="aside-rail">
    ${b('chat', 'chat', '对话', '<span class="rail-badge">3</span>')}
    ${b('agent', 'users', '智能体')}
    ${b('panels', 'panel', '插件面板')}
    <div class="vlabel">对话</div>
  </div>`;
}

function shell(o = {}) {
  return `${titlebar(o)}
  <div class="workspace">
    ${rail(o.rail)}
    ${o.sidebar || sidebarWorkspaces()}
    <main class="main">
      ${o.tabs === null ? '' : (o.tabs || tabstrip())}
      ${o.main || panesDefault()}
      ${o.status || statusbar(o)}
    </main>
    ${o.aside || asideRail()}
  </div>
  ${o.status ? '' : ''}`;
}

/* ---------- sidebar views ---------- */

function sidebarHead(title, count, actions = '') {
  return `<div class="sidebar-head">
    <span class="sidebar-title">${title}</span><span class="sidebar-count">${count}</span>
    <span class="actions">${actions || `<button class="iconbtn">${icon('plus', 'sm')}</button><button class="iconbtn">${icon('filter', 'sm')}</button>`}</span>
  </div>`;
}

function sidebarFoot(text, tone = '') {
  return `<div class="sidebar-foot">${dot(tone || 'ok')}<span>${text}</span></div>`;
}

function sidebarWorkspaces() {
  return `<aside class="sidebar">
    ${sidebarHead('工作区', '10 个 · 33 窗格')}
    <div class="sidebar-body">
      <div class="tree-row on">${icon('chevron-d', 'xs tw')}${icon('folder', 'sm')}<span class="name">term-ide</span><span class="meta">4</span></div>
      <div class="subrow on">${dot('ok')}<span class="name">api</span><span class="meta">2 窗格</span></div>
      <div class="subrow">${dot('warn')}<span class="name">web</span><span class="meta">1 窗格</span></div>
      <div class="subrow"><span class="name">logs</span><span class="meta">1 窗格</span></div>
      <div class="tree-row">${icon('chevron-r', 'xs tw')}${icon('folder', 'sm')}<span class="name">herdr</span><span class="meta">3</span></div>
      <div class="tree-row">${icon('chevron-r', 'xs tw')}${icon('folder', 'sm')}<span class="name">playground</span><span class="meta">2</span></div>
      <div class="tree-row" style="color:var(--fg-subtle)">${icon('more', 'sm')}<span class="name">还有 7 个工作区</span></div>

      <div class="section-label">工作树<span class="actions"><button class="iconbtn" style="width:18px;height:18px">${icon('plus', 'xs')}</button></span></div>
      <div class="wt-row">${icon('branch', 'sm')}<span class="cut">feat/pane-cards</span><span class="tail">~2</span><span class="open"></span></div>
      <div class="wt-row">${icon('branch', 'sm')}<span class="cut">fix/ime-preedit</span><span class="tail">~3</span><span class="open"></span></div>
      <div class="wt-row">${icon('branch', 'sm')}<span class="cut">main</span><span class="tail">~1</span><span class="open"></span></div>

      <div class="block-row">${icon('file', 'sm')}<span class="block-name">.zenix/layout.toml</span><span class="block-note">3 窗格 · 2 split</span>${icon('chevron-r', 'xs')}</div>
    </div>
    ${sidebarFoot('snapshot 推送 · 无轮询')}
  </aside>`;
}

function sidebarAgents() {
  const row = (tone, name, role, meta, extra = '') => `<div class="tree-row">${dot(tone)}<span class="avatar" style="width:18px;height:18px;border-radius:5px;font-size:8px">${name[0].toUpperCase()}</span><span class="name">${name}</span><span class="subtle" style="font-size:10px">${role}</span><span class="meta">${meta}</span>${extra}</div>`;
  return `<aside class="sidebar">
    ${sidebarHead('智能体', '3 个')}
    <div class="sidebar-body">
      <div class="row gap4" style="padding:2px 6px 6px">
        <span class="chip on">全部</span><span class="chip">运行中</span><span class="chip">等待</span>
      </div>
      <div class="section-label">运行中</div>
      ${row('ok', 'planner', 'architect', 'api · 2')}
      ${row('ok', 'builder', 'implementer', 'api · 1')}
      <div class="section-label">等待 / 完成</div>
      ${row('warn', 'reviewer', 'reviewer', 'web · 1')}
      ${row('idle', 'scout', 'explorer', 'logs · 1')}
      <div style="margin:12px 8px 0;padding:8px 10px;border:1px dashed var(--border);border-radius:7px;font-size:11px;color:var(--fg-subtle)">
        点击聚焦窗格；<b style="color:var(--fg-muted)">舰队</b> 查看全部状态与 TAIL。
      </div>
    </div>
    ${sidebarFoot('snapshot 推送 · 无轮询')}
  </aside>`;
}

function sidebarPlugins(selected = 'git-tools') {
  const row = (name, ver, src, tone, actions, disabled) => `<div class="tree-row ${selected === name ? 'on' : ''}" style="${disabled ? 'opacity:.6' : ''}">${dot(tone)}${icon('plug', 'sm')}<span class="name">${name}</span><span class="meta">${ver}</span><span class="badge line" style="height:15px;line-height:15px;padding:0 4px;font-size:8.5px">${src}</span><span class="meta">${actions}</span></div>`;
  return `<aside class="sidebar">
    ${sidebarHead('插件', '4 个')}
    <div class="sidebar-body">
      <div class="input sm" style="margin:2px 4px 7px">${icon('search', 'xs')}<span class="ph">筛选插件…</span></div>
      ${row('git-tools', '0.4.2', 'daemon', 'ok', '3')}
      ${row('docker', '1.1.0', 'local', 'ok', '2')}
      ${row('herdr-notes', '0.2.0', 'local', 'idle', '0', true)}
      ${row('fleet-view', '0.9.1', 'daemon', 'ok', '1')}
      <div class="section-label">来源</div>
      <div style="padding:0 8px;font-size:11px;color:var(--fg-subtle);line-height:1.7">
        <span class="badge line">daemon</span> 注册表<br>
        <span class="badge line">local</span> zenix-ui.toml
      </div>
    </div>
    ${sidebarFoot('plugin.list · 按需读取')}
  </aside>`;
}

function sidebarFleet() {
  const row = (tone, name, role, status, meta) => `<div class="tree-row">${dot(tone)}<span class="name">${name}</span><span class="subtle" style="font-size:10px">${role}</span><span class="meta">${meta} · ${status}</span></div>`;
  return `<aside class="sidebar">
    ${sidebarHead('舰队', '4 个 · 2 运行中')}
    <div class="sidebar-body">
      <div class="row gap4" style="padding:2px 6px 6px"><span class="chip on">全部</span><span class="chip">运行中</span><span class="chip">等待</span></div>
      <div class="section-label">运行中</div>
      ${row('ok', 'planner', 'architect', '12m', 'api')}
      ${row('ok', 'builder', 'implementer', '4m', 'api')}
      <div class="section-label">等待 / 完成</div>
      ${row('warn', 'reviewer', 'reviewer', '2m', 'web')}
      ${row('idle', 'scout', 'explorer', '18m', 'logs')}
      <div style="margin:10px 8px;padding:8px 10px;border:1px solid var(--border);border-radius:7px;background:var(--surface);font-size:10.5px;color:var(--fg-subtle);line-height:1.65">
        状态来自 <span class="mono">pane.agent_status_changed</span> 事件，不轮询。
      </div>
    </div>
    ${sidebarFoot('事件推送 · 无轮询')}
  </aside>`;
}

function sidebarEvents() {
  const chip = (label, on, n) => `<span class="chip ${on ? 'on' : ''}">${label}${n ? ` <span class="mono" style="font-size:9px;color:var(--fg-subtle)">${n}</span>` : ''}</span>`;
  const row = (time, name, subject, tone, on) => `<div class="ev-row ${on ? 'on' : ''}">${dot(tone)}<div class="ev-body"><div class="ev-top"><span>${time}</span><span class="cut">${name}</span></div><div class="ev-subj cut">${subject}</div></div></div>`;
  return `<aside class="sidebar">
    ${sidebarHead('事件', '128 条')}
    <div class="sidebar-body">
      <div class="input sm" style="margin:2px 4px 6px">${icon('search', 'xs')}<span class="ph">搜索事件、对象…</span></div>
      <div class="row" style="flex-wrap:wrap;gap:4px;padding:0 6px 5px">${chip('全部', true)}${chip('智能体', false, 12)}${chip('插件', false, 18)}${chip('工作区·标签', false, 51)}${chip('布局', false, 17)}</div>
      <div class="section-label">事件流<span class="actions"><span class="badge accent" style="height:15px;line-height:15px">live</span></span></div>
      ${row('00:12:04', 'pane.focused', 'term-ide · api · pane 2', 'acc', true)}
      ${row('00:12:01', 'tab.created', 'api · tab 3', 'ok', false)}
      ${row('00:11:58', 'worktree.created', 'feat/ime', 'ok', false)}
      ${row('00:11:52', 'pane.agent_detected', 'planner · api', 'warn', false)}
      ${row('00:11:40', 'workspace.updated', 'term-ide', 'idle', false)}
      ${row('00:11:31', 'tab.focused', 'api', 'idle', false)}
      ${row('00:11:18', 'pane.exited', 'logs · pane 1', 'err', false)}
    </div>
    <div class="sidebar-foot">${dot('ok')}<span>21 个订阅事件</span><span style="margin-left:auto">${icon('external', 'xs')} 契约</span></div>
  </aside>`;
}

/* ---------- tab strip + panes ---------- */

function tabstrip(o = {}) {
  const tab = (name, tone, panes, on) => `<div class="tab ${on ? 'on' : ''}">${tone ? dot(tone) : ''}<span>${name}</span><span class="meta">${panes}</span><span class="close">${icon('x', 'xs')}</span></div>`;
  return `<div class="tabstrip">
    ${tab('api', 'ok', 2, true)}
    ${tab('web', 'warn', 1, false)}
    ${tab('logs', '', 1, false)}
    <div class="actions">
      <button class="iconbtn">${icon('plus', 'sm')}</button>
      <button class="iconbtn">${icon('split', 'sm')}</button>
      <button class="iconbtn">${icon('search', 'sm')}</button>
    </div>
  </div>`;
}

function paneLeft(focused = true) {
  return `<div class="pane ${focused ? 'focus' : ''}">
    <div class="pane-head">${icon('terminal', 'xs')}<span class="pane-title">zsh — api</span><span class="pane-meta">utf-8 · 2/2</span></div>
    <div class="pane-body"><div class="ln"><span class="prompt">❯</span> <span class="path">~/work/term-ide</span> <span class="out">main</span></div>
<div class="ln"><span class="cmd">cargo test -p zenix --lib</span></div>
<div class="ln"><span class="out">   Compiling zenix v0.1.0 (/home/zero/work/...)</span></div>
<div class="ln"><span class="out">    Finished \`test\` profile [unoptimized] in 41.2s</span></div>
<div class="ln"><span class="out">     Running unittests src/lib.rs</span></div>
<div class="ln"><span class="ok">test result: ok. 544 passed; 0 failed; 0 ignored</span></div>
<div class="ln"> </div>
<div class="ln"><span class="prompt">❯</span> <span class="cmd">git status --short</span></div>
<div class="ln"><span class="warn"> M</span> <span class="out">src/shell/ely_theme.rs</span></div>
<div class="ln"><span class="warn">??</span> <span class="out">docs/ui/redesign/</span></div>
<div class="ln"><span class="warn">??</span> <span class="out">src/shell/chat.rs</span></div>
<div class="ln"> </div>
<div class="ln"><span class="prompt">❯</span> <span class="caret" style="display:inline-block;width:6px;height:12px;background:var(--accent);vertical-align:-1px"></span></div>
</div>
  </div>`;
}

function paneRight() {
  return `<div class="pane">
    <div class="pane-head">${icon('terminal', 'xs')}<span class="pane-title">zsh — web</span><span class="pane-meta">utf-8 · 1/1</span></div>
    <div class="pane-body"><div class="ln"><span class="prompt">❯</span> <span class="path">~/work/term-ide/web</span> <span class="out">feat/pane-cards</span></div>
<div class="ln"><span class="cmd">pnpm dev</span></div>
<div class="ln"> </div>
<div class="ln"><span class="ok">  VITE v6.0.7  ready in 214 ms</span></div>
<div class="ln"> </div>
<div class="ln"><span class="out">  ➜  Local:   http://localhost:5173/</span></div>
<div class="ln"><span class="out">  ➜  Network: use --host to expose</span></div>
<div class="ln"><span class="out">  ➜  press h + enter to show help</span></div>
<div class="ln"> </div>
<div class="ln"><span class="warn">[vite]</span> <span class="out">hmr update /src/shell.tsx</span></div>
<div class="ln"><span class="warn">[vite]</span> <span class="out">hmr update /src/panes.tsx</span></div>
<div class="ln"> </div>
<div class="ln"><span class="prompt">❯</span> <span class="caret" style="display:inline-block;width:6px;height:12px;background:var(--accent);vertical-align:-1px"></span></div>
</div>
  </div>`;
}

function panesDefault() {
  return `<div class="panes two">${paneLeft(true)}${paneRight()}</div>`;
}

/* ---------- chat ---------- */

function asideTabs(active = 'chat') {
  return `<div class="aside-head">
    <div class="aside-tab ${active === 'chat' ? 'on' : ''}">${icon('chat', 'xs')}对话<span class="badge accent">3</span></div>
    <div class="aside-tab ${active === 'fleet' ? 'on' : ''}">${icon('gauge', 'xs')}舰队</div>
    <div class="aside-tab ${active === 'panels' ? 'on' : ''}">${icon('panel', 'xs')}面板</div>
    <button class="iconbtn close">${icon('x', 'sm')}</button>
  </div>`;
}

function composer(o = {}) {
  const text = o.text ? esc(o.text) : '<span class="ph">输入消息…（@ 提及，/ 命令）</span>';
  return `<div class="composer">
    <div class="context">${dot('ok')}<span>opencode-go-chat · deepseek-v4.1-flash</span><span class="right"><span>${o.rounds || 4} 轮</span><span>${o.tokens || '18.2k'} tok</span></span></div>
    <div class="field">${text}${o.caret ? '<span class="caret"></span>' : ''}</div>
    <div class="tools">
      <span class="select">${icon('box', 'xs')} deepseek-v4.1-flash ${icon('chevron-d', 'xs')}</span>
      <span class="select">中 ${icon('chevron-d', 'xs')}</span>
      <span class="badge line" style="margin-left:4px">${icon('command', 'xs')} / 命令</span>
      <span class="send ${o.streaming ? 'stop' : ''}">${icon(o.streaming ? 'square' : 'arrow-up', 'sm')}</span>
    </div>
  </div>`;
}

function chatEmpty() {
  return `<div class="chat-scroll" style="align-items:center;justify-content:center;text-align:center;gap:10px">
    <div class="avatar" style="width:34px;height:34px;border-radius:9px">${icon('spark', 'lg')}</div>
    <div style="font-size:16px;font-weight:650;letter-spacing:-.01em">需要我做什么？</div>
    <div class="mini" style="max-width:250px;font-size:11px">直接下达任务；编排器可以启停 daemon 的窗格并读取其输出。</div>
    <div class="row" style="flex-wrap:wrap;gap:6px;justify-content:center;margin-top:2px">
      <span class="chip">${icon('gauge', 'xs')}总结聚焦窗格</span>
      <span class="chip">${icon('users', 'xs')}检查运行中的智能体</span>
      <span class="chip">${icon('layout', 'xs')}检查工作区布局</span>
    </div>
  </div>${composer({ rounds: 0, tokens: '0' })}`;
}

function chatConversation() {
  return `<div class="chat-scroll">
    <div class="msg-user"><div class="who">你 · 刚刚</div>查一下 api 窗格为什么起不来，顺便把测试跑了</div>
    <div class="msg">
      <div class="msg-head"><div class="avatar">编</div><span class="msg-who">zenix 编排器</span><span class="msg-time">刚刚</span></div>
      <div class="msg-body">
        <div class="reasoning">
          <div class="reasoning-head">${icon('spark', 'xs')}<span>思考了 3 秒</span><span style="margin-left:auto">+</span></div>
        </div>
        <div class="toolcard">
          <div class="toolcard-head">${icon('terminal', 'sm')}<span class="tool-name">read_output</span><span class="subtle" style="font-size:10px">api · pane 2</span><span class="badge ok" style="margin-left:auto">完成 · 0.4s</span></div>
          <div class="toolcard-body">panic: could not compile \`zenix\`
error[E0308]: mismatched types
  --&gt; src/shell/chat.rs:418:9
   |
418|   let input = TextInput::new(window, cx);
   |       ^^^^^ expected \`Entity&lt;Input&gt;\`, found \`Entity&lt;TextInput&gt;\`</div>
          <div class="toolcard-foot"><span>参数</span><span>结果</span><span style="margin-left:auto">展开 ${icon('chevron-d', 'xs')}</span></div>
        </div>
        <div class="toolcard">
          <div class="toolcard-head">${icon('zap', 'sm')}<span class="tool-name">spawn_worker</span><span class="subtle" style="font-size:10px">cargo test</span><span class="badge accent" style="margin-left:auto">运行中 · 1.2s</span></div>
          <div class="toolcard-body">worker w-17 → pane api
$ cargo test -p zenix --lib
   Compiling zenix v0.1.0 ...
<div class="ln"><span class="out">test result: ...</span> <span style="display:inline-block;width:6px;height:10px;background:var(--accent);vertical-align:-1px"></span></div></div>
        </div>
        <div class="md-h">问题不在终端，在壳层：</div>
        <p>输入框类型在迁移后变了，<code class="inline">spawn_worker</code> 拿到的还是旧的 <code class="inline">Entity&lt;Input&gt;</code>。终端本身正常，编译错误只是连锁反应。</p>
        <ul>
          <li>把 <code class="inline">chat.rs</code> 的输入换成组件的 <code class="inline">TextInput</code>；</li>
          <li><code class="inline">read_output</code> 已确认 pane 2 的 scrollback 完整；</li>
          <li>测试正在新的 worker 里重跑，预计 40 秒。</li>
        </ul>
        <pre>$ cargo test -p zenix --lib
test result: ok. 544 passed; 0 failed</pre>
      </div>
      <div class="msg-foot">${icon('check', 'xs')} 7.4k tok · 1.8s<span class="actions"><button class="iconbtn" style="width:22px;height:22px">${icon('copy', 'xs')}</button><button class="iconbtn" style="width:22px;height:22px">${icon('refresh', 'xs')}</button></span></div>
    </div>
  </div>${composer({ caret: true })}`;
}

function chatApproval() {
  return `<div class="chat-scroll">
    <div class="msg-user"><div class="who">你 · 刚刚</div>把测试跑起来，失败就修</div>
    <div class="msg">
      <div class="msg-head"><div class="avatar">编</div><span class="msg-who">zenix 编排器</span><span class="msg-time">刚刚</span></div>
      <div class="msg-body">
        <div class="reasoning"><div class="reasoning-head">${icon('spark', 'xs')}<span>思考了 1 秒</span><span style="margin-left:auto">+</span></div></div>
        <div class="toolcard">
          <div class="toolcard-head">${icon('terminal', 'sm')}<span class="tool-name">read_output</span><span class="badge ok" style="margin-left:auto">完成</span></div>
          <div class="toolcard-body">test result: FAILED. 1 failed; 543 passed</div>
        </div>
        <div class="approval">
          <div class="title">${icon('alert', 'sm')}编排器想运行 spawn_worker</div>
          <div class="args">{ "command": "cargo test -p zenix --lib", "pane": "api", "agent": "builder" }</div>
          <div class="actions">
            <span class="btn ghost sm">拒绝</span>
            <span class="btn secondary sm">总是允许</span>
            <span class="btn primary sm">允许一次</span>
          </div>
        </div>
        <p class="muted" style="font-size:11px">这次运行会写入 api 窗格；批准后可随时停止。</p>
      </div>
    </div>
  </div>${composer({ streaming: true })}`;
}

function chatHistory() {
  const row = (title, stamp, on, acts) => `<div class="hist-row ${on ? 'on' : ''}"><span class="t">${title}</span><span class="stamp">${stamp}</span>${acts ? `<span class="acts"><button class="iconbtn" style="width:20px;height:20px">${icon('edit', 'xs')}</button><button class="iconbtn" style="width:20px;height:20px">${icon('trash', 'xs')}</button></span>` : ''}</div>`;
  return `<div class="chat-scroll" style="padding:0">
    <div class="history">
      <div class="history-head"><span>对话</span><span class="badge line">6 个</span><span class="btn sm" style="margin-left:auto">${icon('plus', 'xs')} 新建</span></div>
      ${row('api 窗格启动失败排查', '3 分钟前', true, true)}
      <div class="rename-bar"><div class="input sm grow">${icon('edit', 'xs')}<span>api 窗格启动失败排查</span></div><span class="btn sm primary">保存</span><span class="btn sm ghost">取消</span></div>
      ${row('整理 worktree 清理方案', '今天 14:12', false)}
      ${row('把 layout.toml 同步到仓库', '昨天', false)}
      ${row('插件 argv 审查', '10月24日', false)}
      <div class="confirm-bar"><span>${icon('alert', 'xs')} 删除「插件 argv 审查」？</span><span class="actions"><span class="btn sm ghost">取消</span><span class="btn sm danger solid">删除</span></span></div>
      ${row('组件清单', '10月23日', false)}
      ${row('让 planner 检查事件契约', '10月22日', false)}
    </div>
  </div>${composer({ rounds: 6, tokens: '31.4k' })}`;
}

function asideChat(state = 'empty') {
  const body = state === 'conversation' ? chatConversation() : state === 'approval' ? chatApproval() : state === 'history' ? chatHistory() : chatEmpty();
  return `<aside class="aside">${asideTabs('chat')}<div class="chat">${body}</div></aside>`;
}

function asideFleet(compare = false) {
  const card = (tone, name, role, status, age, strip) => `<div class="agent-card" style="padding:8px;gap:5px">
    <div class="agent-top">${dot(tone)}<span class="agent-name">${name}</span><span class="agent-age">${age}</span></div>
    <div class="agent-pane">${role} · ${status}</div>
    <div class="agent-strip">${strip}</div>
    <div class="agent-foot"><span class="badge ${tone === 'warn' ? 'warn' : tone === 'idle' ? 'line' : 'ok'}" style="height:16px;line-height:16px;font-size:9px">${status}</span><span class="right"><button class="iconbtn" style="width:20px;height:20px">${icon('eye', 'xs')}</button><button class="iconbtn" style="width:20px;height:20px">${icon('terminal', 'xs')}</button></span></div>
  </div>`;
  const strip = (arr) => arr.map((c) => `<i class="${c}"></i>`).join('');
  return `<aside class="aside">
    ${asideTabs('fleet')}
    <div class="chat">
      <div class="fleet-head" style="height:34px;padding:0 10px;border-bottom:1px solid var(--border)">
        <h3 style="font-size:12px">舰队</h3><span class="badge ok">2 运行中</span>
        <span class="right"><span class="chip ${compare ? '' : 'on'}">花名册</span><span class="chip ${compare ? 'on' : ''}">TAIL</span></span>
      </div>
      <div class="fleet-grid" style="padding:8px;gap:7px">
        ${card('ok', 'planner', 'architect', 'working', '12m', strip(['ok','ok','ok','ok','acc','acc','','']))}
        ${card('ok', 'builder', 'implementer', 'working', '4m', strip(['ok','ok','ok','acc','acc','acc','','']))}
        ${card('warn', 'reviewer', 'reviewer', 'waiting', '2m', strip(['ok','ok','ok','warn','warn','warn','','']))}
        ${card('idle', 'scout', 'explorer', 'done', '18m', strip(['ok','ok','ok','ok','ok','ok','ok','ok']))}
      </div>
      ${compare ? `<div class="compare">
        <div class="compare-head">${icon('terminal', 'xs')} TAIL 对比 <span class="subtle mono" style="margin-left:auto">planner · builder</span></div>
        <div class="compare-cols">
          <div class="compare-col"><div class="ch"><span class="dot warn"></span>planner</div>❯ cargo test -p zenix<br>error[E0308]<br>  --&gt; chat.rs:418<br>   |<br>418| let input<br><span class="hit">expected Entity&lt;Input&gt;</span></div>
          <div class="compare-col"><div class="ch"><span class="dot ok"></span>builder</div>❯ cargo test -p zenix<br><span class="hit">test result: ok</span><br>544 passed<br>Finished 41.2s<br>&nbsp;</div>
        </div>
        <div class="compare-foot">${icon('check', 'xs')} 2 处差异 · 尾部 200 行<span style="margin-left:auto">${icon('external', 'xs')} 完整日志</span></div>
      </div>` : `<div class="chat-scroll" style="padding:0 10px 10px;gap:8px">
        <div class="reasoning">
          <div class="reasoning-head">${icon('refresh', 'xs')}<span>最近转场</span><span style="margin-left:auto">4 条</span></div>
          <div style="padding:0 8px 8px;display:flex;flex-direction:column;gap:4px;font-size:10px">
            <div class="row gap6">${dot('warn')}<span class="mono subtle">00:12</span><span>reviewer</span><span class="badge warn" style="margin-left:auto">waiting</span></div>
            <div class="row gap6">${dot('ok')}<span class="mono subtle">00:11</span><span>builder</span><span class="badge ok" style="margin-left:auto">working</span></div>
            <div class="row gap6">${dot('ok')}<span class="mono subtle">00:08</span><span>planner</span><span class="badge ok" style="margin-left:auto">working</span></div>
            <div class="row gap6">${dot('idle')}<span class="mono subtle">23:54</span><span>scout</span><span class="badge line" style="margin-left:auto">done</span></div>
          </div>
        </div>
        <div class="mini" style="font-size:10px;color:var(--fg-subtle)">状态来自 <span class="mono">pane.agent_status_changed</span>；不轮询。</div>
      </div>`}
    </div>
  </aside>`;
}

function asidePanels() {
  return `<aside class="aside">
    ${asideTabs('panels')}
    <div class="chat">
      <div class="chat-scroll">
        <div class="settings-card"><div class="settings-card-head"><div class="settings-card-title">git-tools · 状态</div><div class="settings-card-desc">插件声明 · 10 秒刷新</div></div>
          <div class="settings-row"><div class="label"><div class="t">当前分支</div><div class="n">来自 git status</div></div><span class="badge line mono">feat/pane-cards</span></div>
          <div class="settings-row"><div class="label"><div class="t">未提交改动</div><div class="n">3 个文件</div></div><span class="badge warn">3</span></div>
        </div>
        <div class="settings-card"><div class="settings-card-head"><div class="settings-card-title">docker · 资源</div><div class="settings-card-desc">插件声明 · 30 秒刷新</div></div>
          <div class="settings-row"><div class="label"><div class="t">运行中的容器</div></div><span class="mono">2</span></div>
        </div>
      </div>
    </div>
  </aside>`;
}
