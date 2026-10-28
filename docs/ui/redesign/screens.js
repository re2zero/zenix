/* Screen definitions and the proposal boot: every functional surface, with states. */

/* ---------- palette ---------- */

function paletteOverlay() {
  const row = (ico, label, hint, right, on) => `<div class="pal-row ${on ? 'on' : ''}">${icon(ico, 'sm')}<span class="label">${label}</span><span class="hint">${hint}</span><span class="right">${right}</span></div>`;
  return `<div class="scrim"><div class="palette">
    <div class="palette-input">${icon('search', 'sm')}<span>work</span><span class="caret"></span><span class="esc">${kbd('esc')}</span></div>
    <div class="palette-body">
      <div class="palette-section">命令<span class="count">3</span></div>
      ${row('branch', '新建工作树', '在 ~/work/term-ide~2 创建', `${kbd('ctrl')}${kbd('b')}${kbd('⇧')}${kbd('N')}`, true)}
      ${row('grid', '新建工作区', '从当前分支开始', `${kbd('⌘')}${kbd('⇧')}${kbd('N')}`, false)}
      ${row('terminal', '新建标签', '在当前工作区打开', `${kbd('⌘')}${kbd('T')}`, false)}
      <div class="palette-section">工作区<span class="count">2</span></div>
      ${row('folder', 'term-ide', '4 标签 · 8 窗格', badge('当前', 'line'), false)}
      ${row('folder', 'term-ide~2', 'feat/pane-cards · 2 窗格', '', false)}
      <div class="palette-section">插件动作<span class="count">2</span></div>
      ${row('plug', 'git-tools · sync', '同步子模块与状态', badge('daemon', 'violet'), false)}
      ${row('plug', 'docker · compose up', '启动 compose 服务', badge('local', 'info'), false)}
    </div>
    <div class="palette-foot"><span>↑↓ 选择</span><span>↵ 执行</span><span>esc 关闭</span><span class="right">6 个匹配 · 2 个隐藏</span></div>
  </div></div>`;
}

/* ---------- plugin surfaces ---------- */

function pluginTabs(active = '概览') {
  return `<div style="display:flex;gap:3px;padding:6px 10px 0;border-bottom:1px solid var(--border)">
    ${['概览', '动作', '面板', '日志'].map((t) => `<span class="plugin-tab ${t === active ? 'on' : ''}">${t}</span>`).join('')}
  </div>`;
}

function pluginDrawer() {
  return `<div class="drawer w480">
    <div class="drawer-head">${icon('plug')}<span style="font-size:12.5px;font-weight:650">git-tools</span><span class="badge line">v0.4.2</span><span class="badge violet">daemon</span><span style="margin-left:auto"></span><button class="iconbtn">${icon('x', 'sm')}</button></div>
    ${pluginTabs('概览')}
    <div class="drawer-body">
      <div class="kv-card">
        <div class="kv-head">合同</div>
        <div class="kv-row"><span class="k">manifest</span><span class="v">~/.local/share/herdr/plugins/git-tools/zenix-ui.toml</span></div>
        <div class="kv-row"><span class="k">state</span><span class="v">~/.local/state/herdr/plugins/git-tools</span></div>
        <div class="kv-row"><span class="k">config</span><span class="v">~/.config/herdr/plugins/git-tools</span></div>
        <div class="kv-row"><span class="k">refresh</span><span class="v">10s · 仅可见时读取</span></div>
      </div>
      <div class="kv-card">
        <div class="kv-head">命令与风险</div>
        <div class="argv-row"><span class="cmd">git status --porcelain=v2 --branch</span><span class="badge ok">低</span></div>
        <div class="argv-row"><span class="cmd">git submodule update --init --recursive</span><span class="badge warn">中</span></div>
        <div class="argv-row"><span class="cmd">sh -c "$ZENIX_STATE/refresh.sh"</span><span class="badge violet">插件内</span></div>
      </div>
      <div class="row gap6">
        <div class="kv-card grow"><div class="kv-head">声明的 UI</div>
          <div class="kv-row"><span class="k">sidebar</span><span class="v">分支 · 未提交改动</span></div>
          <div class="kv-row"><span class="k">status</span><span class="v">git 状态段</span></div>
          <div class="kv-row"><span class="k">action</span><span class="v">sync · status · log</span></div>
        </div>
        <div class="kv-card grow"><div class="kv-head">动作</div>
          <div class="kv-row"><span class="k">sync</span><span class="v mono">ctrl b y</span></div>
          <div class="kv-row"><span class="k">status</span><span class="v mono">palette</span></div>
          <div class="kv-row"><span class="k">log</span><span class="v mono">palette</span></div>
        </div>
      </div>
      <div class="kv-card"><div class="kv-head">日志 <span class="badge line" style="margin-left:4px">最近 3 条</span></div>
        <div class="kv-row"><span class="k mono">00:12</span><span class="v">sync ok · 3 actions</span></div>
        <div class="kv-row"><span class="k mono">00:10</span><span class="v">status read · 42ms</span></div>
        <div class="kv-row"><span class="k mono">23:58</span><span class="v warn">submodule update: caution</span></div>
      </div>
    </div>
    <div class="drawer-foot"><span class="badge ok">已启用</span><span class="switch on"></span><span style="margin-left:auto;display:flex;gap:5px"><span class="btn sm ghost">打开目录</span><span class="btn sm danger">卸载</span></span></div>
  </div>`;
}

function trustModal() {
  return `<div class="scrim" style="z-index:22"><div class="trust-card">
    <div class="trust-head">
      <div class="row gap6">${icon('shield', 'lg')}<h3>审查插件 git-tools</h3><span class="badge violet">local</span><span class="badge line" style="margin-left:auto">v0.4.2</span></div>
      <p class="mini" style="margin:4px 0 0">链接前，zenix 读取插件目录里的每一个 argv，并按风险等级标注；链接后可随时停用。</p>
    </div>
    <div class="trust-body">
      <div class="kv-card"><div class="kv-row"><span class="k">manifest</span><span class="v">~/work/plugins/git-tools/zenix-ui.toml</span></div></div>
      <div class="kv-card">
        <div class="argv-row"><span class="cmd">git status --porcelain=v2 --branch</span><span class="badge ok">低风险</span></div>
        <div class="argv-row"><span class="cmd">git submodule update --init --recursive</span><span class="badge warn">中风险 · 网络</span></div>
        <div class="argv-row"><span class="cmd">sh -c "$ZENIX_STATE/refresh.sh"</span><span class="badge violet">插件内脚本</span></div>
      </div>
      <div class="row gap6" style="align-items:center;font-size:11px;color:var(--fg-muted)">
        <span style="width:15px;height:15px;border:1px solid var(--accent);border-radius:3px;display:inline-grid;place-items:center;color:var(--accent)">${icon('check', 'xs')}</span>
        我已阅读并理解上述命令
        <span class="mini" style="margin-left:auto">2 条中风险 · 1 条需网络</span>
      </div>
    </div>
    <div class="trust-foot"><span class="subtle" style="font-size:10.5px">路径按 herdr v0.9.5 的算法计算</span><span class="right"><span class="btn ghost">取消</span><span class="btn primary">信任并链接</span></span></div>
  </div></div>`;
}

/* ---------- events ---------- */

function eventDrawer() {
  return `<div class="drawer w420">
    <div class="drawer-head">${icon('pulse')}<span class="mono" style="font-size:10.5px">pane.focused</span><span class="badge warn" style="margin-left:auto">seq 48213</span><button class="iconbtn">${icon('x', 'sm')}</button></div>
    <div class="inspector-tabs"><span class="chip on">JSON</span><span class="chip">原始</span><span class="chip">契约</span></div>
    <div class="drawer-body">
      <div class="json">{
  <span class="key">"seq"</span>: <span class="num">48213</span>,
  <span class="key">"event"</span>: <span class="str">"pane.focused"</span>,
  <span class="key">"workspace"</span>: <span class="str">"term-ide"</span>,
  <span class="key">"pane"</span>: <span class="str">"pane-2"</span>,
  <span class="key">"at"</span>: <span class="str">"2026-10-27T00:12:04.318Z"</span>
}</div>
      <div class="kv-card"><div class="kv-head">元数据</div>
        <div class="kv-row"><span class="k">来源</span><span class="v">endpoint · 已订阅</span></div>
        <div class="kv-row"><span class="k">类别</span><span class="v">窗格 / 焦点</span></div>
        <div class="kv-row"><span class="k">延迟</span><span class="v">12 ms</span></div>
      </div>
      <div class="mini" style="font-size:10.5px">状态变化只来自推送；JSON lane 用于 <span class="mono">plugin.*</span> / <span class="mono">worktree.*</span> 的按需读取。</div>
    </div>
    <div class="drawer-foot"><span class="mono subtle" style="font-size:9.5px">00:12:04.318</span><span style="margin-left:auto" class="btn sm ghost">${icon('copy', 'xs')} 复制 JSON</span></div>
  </div>`;
}

const EVENT_NAMES = [
  ['workspace.created', 'workspace_created'], ['workspace.updated', 'workspace_updated'],
  ['workspace.closed', 'workspace_closed'], ['workspace.renamed', 'workspace_renamed'],
  ['workspace.moved', 'workspace_moved'], ['workspace.reordered', 'workspace_reordered'],
  ['workspace.focused', 'workspace_focused'], ['worktree.created', 'worktree_created'],
  ['worktree.opened', 'worktree_opened'], ['worktree.removed', 'worktree_removed'],
  ['tab.created', 'tab_created'], ['tab.closed', 'tab_closed'],
  ['tab.renamed', 'tab_renamed'], ['tab.moved', 'tab_moved'],
  ['tab.focused', 'tab_focused'], ['pane.created', 'pane_created'],
  ['pane.closed', 'pane_closed'], ['pane.focused', 'pane_focused'],
  ['pane.moved', 'pane_moved'], ['pane.exited', 'pane_exited'],
  ['pane.agent_detected', 'pane_agent_detected'],
];

function contractModal() {
  return `<div class="scrim" style="z-index:22"><div class="contract">
    <div class="contract-head"><div class="row gap6">${icon('link')}<span style="font-size:14px;font-weight:650">事件契约</span><span class="badge accent">21 个 SUBSCRIBED</span><button class="iconbtn" style="margin-left:auto">${icon('x', 'sm')}</button></div><p class="mini" style="margin:4px 0 0">一条订阅；同一个事件有 dotted 与 wire（snake_case）两种拼写。</p></div>
    <div class="contract-body" style="grid-template-columns:1fr 1fr 1fr">
      ${EVENT_NAMES.map(([dotted, wire]) => `<div><div class="mono">${dotted}</div><div class="subtle" style="font-size:9px">${wire}</div></div>`).join('')}
    </div>
    <div class="contract-foot"><span>JSON lane：plugin.* / worktree.* / layout.* 按需读取</span><span style="margin-left:auto" class="btn sm ghost">${icon('copy', 'xs')} 复制清单</span></div>
  </div></div>`;
}

/* ---------- layout ---------- */

function codeLine(n, html) {
  return `<div class="code-line"><span class="n">${n}</span><span class="c">${html}</span></div>`;
}

function layoutModal() {
  return `<div class="scrim"><div class="layout-modal">
    <div class="layout-head">${icon('file')}<span class="mono" style="font-size:11.5px">.zenix/layout.toml</span><span class="badge ok">已同步</span><span class="mini" style="margin-left:4px">~/work/term-ide/.zenix/layout.toml</span><span class="right"><span class="btn sm ghost">${icon('copy', 'xs')} 复制路径</span><span class="btn sm secondary">${icon('download', 'xs')} 导出</span><span class="btn sm primary">${icon('play', 'xs')} 应用</span><button class="iconbtn">${icon('x', 'sm')}</button></span></div>
    <div class="layout-cols">
      <div class="layout-code">
        ${codeLine(1, '<span class="com"># 项目自带的布局模板 · 提交到仓库</span>')}
        ${codeLine(2, '<span class="k">tab_label</span> = <span class="s">"zenix"</span>')}
        ${codeLine(3, '<span class="k">focus</span> = <span class="t">true</span>')}
        ${codeLine(4, '')}
        ${codeLine(5, '<span class="k">[root]</span>')}
        ${codeLine(6, '<span class="k">type</span> = <span class="s">"split"</span>')}
        ${codeLine(7, '<span class="k">direction</span> = <span class="s">"right"</span>')}
        ${codeLine(8, '<span class="k">ratio</span> = <span class="t">0.62</span>')}
        ${codeLine(9, '')}
        ${codeLine(10, '<span class="k">[root.first]</span>')}
        ${codeLine(11, '<span class="k">type</span> = <span class="s">"pane"</span>')}
        ${codeLine(12, '<span class="k">label</span> = <span class="s">"planner"</span>')}
        ${codeLine(13, '<span class="k">cwd</span> = <span class="s">"~/work/term-ide"</span>')}
        ${codeLine(14, '')}
        ${codeLine(15, '<span class="k">[root.second]</span>')}
        ${codeLine(16, '<span class="k">type</span> = <span class="s">"split"</span>')}
        ${codeLine(17, '<span class="k">direction</span> = <span class="s">"down"</span>')}
        ${codeLine(18, '<span class="k">ratio</span> = <span class="t">0.5</span>')}
        ${codeLine(19, '')}
        ${codeLine(20, '<span class="k">[root.second.first]</span>')}
        ${codeLine(21, '<span class="k">type</span> = <span class="s">"pane"</span>')}
        ${codeLine(22, '<span class="k">label</span> = <span class="s">"builder"</span>')}
        ${codeLine(23, '')}
        ${codeLine(24, '<span class="k">[root.second.second]</span>')}
        ${codeLine(25, '<span class="k">type</span> = <span class="s">"pane"</span>')}
        ${codeLine(26, '<span class="k">label</span> = <span class="s">"reviewer"</span>')}
      </div>
      <div class="layout-side">
        <div class="mini" style="font-size:10px;color:var(--fg-subtle);text-transform:uppercase;letter-spacing:.08em">split 预览</div>
        <div class="layout-diagram">
          <div class="split-box">
            <div class="split-box v" style="flex:.62">
              <div class="leaf on">planner</div>
              <div class="leaf">builder</div>
            </div>
            <div class="leaf" style="flex:.38">reviewer</div>
          </div>
        </div>
        <div class="layout-facts">
          <div class="fact"><div class="n">3</div><div class="l">PANE</div></div>
          <div class="fact"><div class="n">2</div><div class="l">SPLIT</div></div>
          <div class="fact"><div class="n">v1</div><div class="l">SCHEMA</div></div>
          <div class="fact"><div class="n">8px</div><div class="l">GAP</div></div>
        </div>
        <div class="callout" style="margin:0;font-size:11px;padding:8px 10px">应用会新建一个标签页；<b>不是幂等</b>，已有窗格不会被替换。文件不写 pane_id。</div>
        <div class="mini" style="font-size:10px">从窗格 cwd 向上最多 12 层找项目根；同一仓库共享这份模板。</div>
      </div>
    </div>
    <div class="drawer-foot"><span class="subtle" style="font-size:10px">写入 <span class="mono">.zenix/layout.toml</span> · 随仓库提交</span><span style="margin-left:auto;display:flex;gap:6px"><span class="btn sm ghost">关闭</span><span class="btn sm primary">应用模板</span></span></div>
  </div></div>`;
}

function newWorktreeModal() {
  return `<div class="scrim"><div class="modal-card">
    <div class="modal-head"><h3>新建工作树</h3><p>从 <span class="mono">term-ide</span> 的 <span class="mono">main</span> 创建，路径由 daemon 命名。</p></div>
    <div class="modal-body">
      <div class="field-row"><label>分支名</label><div class="input focus">${icon('branch', 'sm')}<span>feat/pane-cards</span><span class="caret" style="display:inline-block;width:1px;height:14px;background:var(--accent)"></span></div></div>
      <div class="field-row"><label>基准分支</label><div class="select" style="width:100%;height:30px">${icon('branch', 'sm')} main ${icon('chevron-d', 'xs', )}<span style="margin-left:auto"></span></div></div>
      <div class="field-row"><label>将创建</label><div class="input sm" style="background:var(--sunken);color:var(--fg-muted)"><span class="mono">~/work/term-ide~2</span><span class="badge ok" style="margin-left:auto">可用</span></div></div>
      <div class="mini" style="font-size:10.5px">工作树会出现在侧栏 WORKTREES 块；创建后可直接打开。</div>
    </div>
    <div class="modal-foot"><span class="badge line">git worktree</span><span class="right"><span class="btn ghost">取消</span><span class="btn primary">创建</span></span></div>
  </div></div>`;
}

function removeWorktreeModal() {
  return `<div class="scrim"><div class="modal-card">
    <div class="modal-head"><h3>移除工作树？</h3><p><span class="mono">feat/ime-preedit</span> · <span class="mono">~/work/term-ide~3</span></p></div>
    <div class="modal-body">
      <div class="approval" style="border-color:color-mix(in srgb, var(--danger) 40%, transparent);background:color-mix(in srgb, var(--danger) 6%, transparent)">
        <div class="title" style="color:var(--danger)">${icon('alert', 'sm')}有未提交的改动</div>
        <div class="mini" style="font-size:11px">2 个修改、1 个未跟踪文件。普通移除会被 daemon 拒绝；强制移除会连同工作树目录一起删除。</div>
      </div>
      <div class="row" style="justify-content:space-between"><span style="font-size:11.5px">强制移除</span><span class="mini" style="font-size:10px;margin-left:auto;margin-right:8px">需先打开</span><span class="switch"></span></div>
    </div>
    <div class="modal-foot"><span class="mini" style="font-size:10px">此操作不可撤销</span><span class="right"><span class="btn ghost">取消</span><span class="btn danger solid" style="opacity:.45">移除工作树</span></span></div>
  </div></div>`;
}

/* ---------- settings ---------- */

const SETTINGS_PAGES = [
  ['appearance', '外观', 'palette'],
  ['terminal', '终端', 'terminal'],
  ['ai', 'AI', 'spark'],
  ['keys', '按键', 'command'],
  ['plugins', '插件', 'plug'],
  ['mcp', 'MCP', 'link'],
  ['about', '关于', 'info'],
];

function settingsNav(active) {
  const item = ([id, label, ico]) => `<div class="item ${active === id ? 'on' : ''}">${icon(ico, 'sm')}<span>${label}</span></div>`;
  const find = (id) => SETTINGS_PAGES.find((p) => p[0] === id);
  return `<div class="settings-nav">
    <div class="input sm" style="margin:0 4px 8px">${icon('search', 'xs')}<span class="ph">搜索设置…</span><span class="kbd" style="margin-left:auto">⌘F</span></div>
    <div class="group">应用</div>
    ${item(find('appearance'))}${item(find('terminal'))}${item(find('ai'))}
    <div class="group">扩展</div>
    ${item(find('keys'))}${item(find('plugins'))}${item(find('mcp'))}
    <div class="group">关于</div>
    ${item(find('about'))}
  </div>`;
}

function settingsCard(title, desc, rows) {
  return `<div class="settings-card">
    <div class="settings-card-head"><div class="settings-card-title">${title}</div>${desc ? `<div class="settings-card-desc">${desc}</div>` : ''}</div>
    ${rows}
  </div>`;
}

function settingRow(title, note, control) {
  return `<div class="settings-row"><div class="label"><div class="t">${title}</div>${note ? `<div class="n">${note}</div>` : ''}</div><div class="control">${control}</div></div>`;
}

const segmented = (items, on) => `<div class="segmented">${items.map((t) => `<span class="seg ${t === on ? 'on' : ''}">${t}</span>`).join('')}</div>`;
const select = (label) => `<span class="select">${label} ${icon('chevron-d', 'xs')}</span>`;

function settingsPage(page) {
  if (page === 'appearance') {
    return `<div class="settings-head"><div><h4>外观</h4><p>四层主题配置 + 字体与布局；改动立即写入 config.json。</p></div><div class="right"><span class="btn sm ghost">恢复本页默认</span></div></div>
      ${settingsCard('主题', '四层只改颜色与代码语法，不重建组件。', `
        ${settingRow('模式', '跟随系统时即时生效。', segmented(['深色', '浅色', '跟随系统'], '深色'))}
        ${settingRow('强调色', '只覆盖强调色；默认是单色。', `<div class="accent-presets">
          <span class="accent-dot on" title="默认" style="background:var(--fg)"></span>
          <span class="accent-dot" title="蓝" style="background:#5182c1"></span>
          <span class="accent-dot" title="青" style="background:#009589"></span>
          <span class="accent-dot" title="琥珀" style="background:#aa732b"></span>
          <span class="accent-dot" title="玫瑰" style="background:#b8636b"></span>
          <span class="accent-dot" title="紫" style="background:#8572bb"></span>
          <span class="accent-dot" title="绿" style="background:#539156"></span>
          <span class="accent-dot custom" title="自定义颜色">+</span>
        </div>`)}
        ${settingRow('代码主题', '只影响代码与 markdown 代码块。', `<div class="syntax-picks">
          <span class="syntax-pick on"><span class="dots"><i style="background:#b8636b"></i><i style="background:#539156"></i><i style="background:#5182c1"></i></span>标准</span>
          <span class="syntax-pick"><span class="dots"><i style="background:#8572bb"></i><i style="background:#aa732b"></i><i style="background:#009589"></i></span>静谧</span>
          <span class="syntax-pick"><span class="dots"><i style="background:#8a6a4f"></i><i style="background:#6f7b52"></i><i style="background:#5a6f8a"></i></span>纸面</span>
        </div>`)}
        ${settingRow('自定义主题', '可选：导入或编辑一份调色板，不改组件。', `<span class="btn sm ghost">导入 JSON</span><span class="btn sm ghost">编辑</span>`)}
      `)}
      ${settingsCard('字体', '界面与终端分开设置；字体已编译进二进制。', `
        ${settingRow('界面字体', '中文回退到 Noto Sans SC。', select('Inter'))}
        ${settingRow('等宽字体', '终端、代码与 kbd 都用它。', select('Lilex'))}
      `)}
      ${settingsCard('界面', '这些值只影响客户端布局，不写回 daemon 的 session。', `
        ${settingRow('密度', '影响控件高度与行距，不改字号。', segmented(['紧凑', '标准', '舒适'], '标准'))}
        ${settingRow('面板宽度', '窄于 1180 自动折叠对话面板。', `<span class="row gap6">${select('侧栏 260 px')}${select('对话 372 px')}</span>`)}
        ${settingRow('终端跟随浅色模式', '关闭则终端保持深色，作为视觉锚点。', `<span class="switch on"></span>`)}
      `)}`;
  }
  if (page === 'terminal') {
    return `<div class="settings-head"><div><h4>终端</h4><p>渲染在客户端完成；这些设置只影响绘制与输入。</p></div><div class="right"><span class="btn sm ghost">恢复本页默认</span></div></div>
      ${settingsCard('排版', '等宽字体与单元格尺寸。', `
        ${settingRow('字号', '按 1.58 行高计算单元格。', select('11.5 px'))}
        ${settingRow('光标', '', segmented(['方块', '竖线', '下划线'], '方块'))}
        ${settingRow('Scrollback', '每个窗格保留的行数。', select('10 000 行'))}
      `)}
      ${settingsCard('配色', '终端直接使用内置 ANSI 16 色；不单独维护调色板。', `
        ${settingRow('调色板', '跟随深色 / 浅色模式；强调色不影响 ANSI。', select('跟随模式'))}
        ${settingRow('OSC-8 链接', '按住 Ctrl 点击打开。', `<span class="switch on"></span>`)}
        ${settingRow('响铃', '后台窗格响铃时提示。', `<span class="switch on"></span>`)}
      `)}
      ${settingsCard('选择与复制', '本地选择在客户端完成，不经过 daemon。', `
        ${settingRow('选中即复制', '', `<span class="switch"></span>`)}
        ${settingRow('右键粘贴', '', `<span class="switch on"></span>`)}
        ${settingRow('显示滚动条', '', `<span class="switch on"></span>`)}
      `)}`;
  }
  if (page === 'ai') {
    return `<div class="settings-head"><div><h4>AI</h4><p>编排器的模型、上下文与审批策略。</p></div><div class="right"><span class="btn sm ghost">恢复本页默认</span></div></div>
      ${settingsCard('模型', 'OpenAI 兼容端点；密钥只存在本地。', `
        ${settingRow('提供方', '', select('opencode-go-chat'))}
        ${settingRow('端点', '', `<span class="input sm" style="width:280px"><span class="mono">https://api.opencode.ai/v1</span></span>`)}
        ${settingRow('模型', '', select('deepseek-v4.1-flash'))}
        ${settingRow('推理强度', '', segmented(['低', '中', '高'], '中'))}
        ${settingRow('API 密钥', '已配置 · 更新于 10月20日', `<span class="badge ok">已配置</span><span class="btn sm ghost">更换</span>`)}
      `)}
      ${settingsCard('上下文', '超过阈值时先压缩，再继续。', `
        ${settingRow('压缩阈值', '', select('70%'))}
        ${settingRow('最大工具轮次', '', select('8'))}
        ${settingRow('系统提示', '只读；由角色目录生成。', `<span class="btn sm ghost">预览</span>`)}
      `)}
      ${settingsCard('对话', '', `
        ${settingRow('流式输出', '', `<span class="switch on"></span>`)}
        ${settingRow('发送键', '', segmented(['Enter', '⌘Enter'], 'Enter'))}
        ${settingRow('工具审批', '', segmented(['每次询问', '安全工具自动', '全部允许'], '每次询问'))}
      `)}`;
  }
  if (page === 'keys') {
    const keyRow = (name, keys) => `<div class="settings-row"><div class="label"><div class="t">${name}</div></div><div class="control">${keys}</div></div>`;
    return `<div class="settings-head"><div><h4>按键</h4><p>按键由命令目录生成；这里只覆盖冲突与显示。</p></div><div class="right"><span class="btn sm ghost">恢复本页默认</span></div></div>
      ${settingsCard('命令目录', '同一份目录驱动命令面板、快捷键与按键页。', `
        <div class="settings-row"><div class="input sm grow" style="flex:1">${icon('search', 'xs')}<span class="ph">搜索命令…</span></div></div>
        ${keyRow('新建工作区', `${kbd('⌘')}${kbd('⇧')}${kbd('N')}`)}
        ${keyRow('新建标签', `${kbd('⌘')}${kbd('T')}`)}
        ${keyRow('向右分屏', `${kbd('⌘')}${kbd('D')}`)}
        ${keyRow('关闭窗格', `${kbd('⌘')}${kbd('W')}`)}
        ${keyRow('命令面板', `${kbd('⌘')}${kbd('⇧')}${kbd('P')}`)}
        ${keyRow('切换对话面板', `${kbd('⌘')}${kbd('⇧')}${kbd('A')}`)}
      `)}
      ${settingsCard('前缀键', 'bare-key 插件动作排在 prefix 之后，避免吞掉输入。', `
        ${settingRow('前缀', '', `${kbd('ctrl')}${kbd('b')}<span class="btn sm ghost">录制</span>`)}
        ${settingRow('前缀状态', '按下后 1.5 秒内等待下一个键。', `<span class="badge line">等待中</span>`)}
      `)}`;
  }
  if (page === 'plugins') {
    return `<div class="settings-head"><div><h4>插件</h4><p>宿主事实与插件声明的设置；不改客户端不能执行的东西。</p></div><div class="right"><span class="btn sm ghost">打开插件目录</span></div></div>
      ${settingsCard('插件宿主', '', `
        ${settingRow('已安装 / 已启用', '', `<span class="badge line">4 / 3</span>`)}
        ${settingRow('根目录', '', `<span class="mono" style="font-size:10px">~/.local/share/herdr/plugins</span>`)}
        ${settingRow('合同版本', 'zenix-ui.toml', `<span class="badge line mono">v1</span>`)}
        ${settingRow('链接前审查', '读取每个 argv 并标注风险。', `<span class="switch on"></span>`)}
      `)}
      ${settingsCard('git-tools 设置', '插件声明 · 只读页；改动写入插件自己的配置。', `
        ${settingRow('刷新间隔', '仅可见时读取。', select('10 s'))}
        ${settingRow('状态段', '显示在底部状态栏。', `<span class="switch on"></span>`)}
        ${settingRow('子模块同步', '中风险命令，运行前询问。', segmented(['询问', '允许'], '询问'))}
      `)}
      ${settingsCard('安全', '卸载会先走确认；停用后动作不再出现在命令面板。', `
        ${settingRow('卸载确认', '两步，避免误触。', `<span class="switch on"></span>`)}
        ${settingRow('插件日志', '内存环形缓冲，重启清空。', `<span class="badge line">最近 200 条</span>`)}
      `)}`;
  }
  if (page === 'mcp') {
    const tool = (name, note) => `<div class="mcp-tool"><div class="name">${name}</div><div class="note">${note}</div></div>`;
    return `<div class="settings-head"><div><h4>MCP</h4><p>编排服务与反向桥；这里只陈述事实，不提供假开关。</p></div><div class="right"><span class="badge ok">运行中</span></div></div>
      ${settingsCard('编排服务', '', `
        ${settingRow('状态', '本地 socket，随窗口启停。', `<span class="badge ok">listening</span>`)}
        ${settingRow('Socket', '', `<span class="mono" style="font-size:10px">~/.config/zenix/mcp-orchestrator.sock</span>`)}
        ${settingRow('工具', '编排器自身的 6 个工具。', `<span class="badge line">6</span>`)}
        ${settingRow('插件工具', '每个插件 action 一个工具。', `<span class="badge line">9</span>`)}
      `)}
      ${settingsCard('反向桥', '给没有 shell 的 agent 一个入口。', `
        ${settingRow('命令', '', `<span class="mono" style="font-size:10px">zenix mcp-agent</span><span class="btn sm ghost">复制</span>`)}
        ${settingRow('工具', '', `<span class="badge line mono">agent_send</span>`)}
      `)}
      ${settingsCard('工具清单', '', `
        <div class="mcp-tools">
          ${tool('spawn_worker', '创建 worker 窗格')}
          ${tool('send_text', '发送文本指令')}
          ${tool('read_output', '读取最新输出')}
          ${tool('wait_for_output', '等待输出匹配')}
          ${tool('list_workers', '列出运行中的 worker')}
          ${tool('close_worker', '关闭 worker 窗格')}
        </div>
      `)}`;
  }
  return `<div class="settings-head"><div><h4>关于</h4><p>版本、许可与链接。</p></div></div>
    ${settingsCard('版本', '', `
      ${settingRow('zenix', '', `<span class="mono">0.1.0 · debug</span>`)}
      ${settingRow('herdr', '固定版本，SHA-256 校验', `<span class="badge ok">0.9.5 · match</span>`)}
      ${settingRow('UI', '', `<span class="mono" style="font-size:10px">gpui-pre 0.3.10 · 组件库 0.2.0</span>`)}
    `)}
    ${settingsCard('许可', '', `
      ${settingRow('zenix', '', `<span class="mono">GPL-3.0-or-later</span>`)}
      ${settingRow('herdr', '', `<span class="mono">Apache-2.0</span>`)}
      ${settingRow('组件库', '', `<span class="mono">MIT</span>`)}
      ${settingRow('Lilex', '', `<span class="mono">SIL OFL-1.1</span>`)}
    `)}
    ${settingsCard('链接', '', `
      ${settingRow('仓库', '', `<span class="mono" style="font-size:10px">github.com/zenix-dev/zenix</span>`)}
      ${settingRow('文档', '', `<span class="mono" style="font-size:10px">docs/</span>`)}
    `)}`;
}

function settingsDialog(page) {
  return `<div class="scrim"><div class="dialog">
    <div class="dialog-head"><h3>设置</h3><span class="path">~/.config/zenix/config.json</span><span class="right"><span class="iconbtn">${icon('search', 'sm')}</span><button class="iconbtn">${icon('x', 'sm')}</button></span></div>
    <div class="dialog-body">${settingsNav(page)}<div class="settings-content">${settingsPage(page)}</div></div>
    <div class="settings-foot"><span class="save">${dot('ok')} 已保存 · 写入即生效</span><span>config.json</span><span class="right"><span class="btn sm secondary">关闭</span></span></div>
  </div></div>`;
}

/* ---------- empty ---------- */

function emptyMain() {
  return `<div class="empty-wrap">
    <div class="empty-title">${icon('spark', 'lg')}从第一个工作区开始</div>
    <div class="empty-sub">还没有工作区。新建一个，或打开项目里的 <span class="mono">.zenix/layout.toml</span>；daemon 连上后侧栏会自动列出已有的。</div>
    <div class="empty-grid">
      <div class="entry-card"><div class="entry-ico">${icon('grid', 'lg')}</div><div><div class="t">新建工作区</div><div class="n">从当前目录开始一个会话</div></div><span class="k">${kbd('⌘')}${kbd('⇧')}${kbd('N')}</span></div>
      <div class="entry-card"><div class="entry-ico">${icon('file', 'lg')}</div><div><div class="t">打开布局</div><div class="n">恢复项目模板里的标签</div></div></div>
      <div class="entry-card"><div class="entry-ico">${icon('plug', 'lg')}</div><div><div class="t">浏览插件</div><div class="n">看已安装的 action 与面板</div></div></div>
      <div class="entry-card"><div class="entry-ico">${icon('terminal', 'lg')}</div><div><div class="t">启动 daemon</div><div class="n">首次运行会播种 herdr</div></div></div>
    </div>
    <div class="daemon-card">${icon('alert', 'sm')}<div style="flex:1"><div style="font-size:11.5px;font-weight:600">daemon 未连接</div><div class="mini" style="font-size:10.5px">zenix 会从 <span class="mono">res/herdr</span> 播种到 <span class="mono">~/.local/bin/herdr</span>；也可以手动启动。</div></div><span class="btn sm secondary">查看日志</span><span class="btn sm primary">启动 daemon</span></div>
  </div>`;
}

/* ---------- screens ---------- */

const SCREENS = [
  {
    id: 'shell', num: '01', title: '工作区外壳', lede: '终端是主区域；侧栏、对话与状态栏都围绕它。标题栏是身份条，命令面板从中间进入。',
    states: [{ id: 'workspaces', label: '工作区' }, { id: 'agents', label: '智能体' }, { id: 'plugins', label: '插件' }],
    render(v) {
      const side = v === 'agents' ? sidebarAgents() : v === 'plugins' ? sidebarPlugins() : sidebarWorkspaces();
      const pins = pin(1, 960, 8, '标题栏：品牌、工作区药丸（分支 + 连接状态）、命令面板入口、面板开关。') +
        pin(2, 14, 112, 'Rail：5 个侧栏视图 + 设置；选中项左侧 2px 强调条，智能体带未读角标。') +
        pin(3, 250, 118, '侧栏 260px：视图头 36px，行 28px，选中行强调条；脚注写明数据来源。') +
        pin(4, 300, 50, '标签条 42px：标签 28px，带智能体状态点与窗格数；右侧新建/分屏/搜索。') +
        pin(5, 440, 450, '窗格卡片：8px 间距、8px 圆角、1px 发丝线；聚焦卡片换 accent 边框。') +
        pin(6, 1414, 116, '对话面板折叠为 34px 竖轨；角标显示未读，展开后是 Chat / Fleet / Panels。') +
        pin(7, 660, 884, '状态栏 26px：连接、工作区/标签/窗格计数、分支、诊断、前缀提示。');
      return shell({ rail: 'workspaces', sidebar: side, main: panesDefault(), aside: asideRail() }) + pins;
    },
    notes(v) {
      if (v === 'agents') return {
        flow: ['智能体视图从 snapshot 的 <b>agents + agent_order</b> 构建，不轮询。', '行内状态点跟随 <b>pane.agent_status_changed</b>；点击行聚焦对应窗格。', '悬停显示角色与窗格；点击右侧箭头跳到舰队视图。'],
        states: ['运行中 / 等待 / 完成 / 错误四色；等待态用 warn，不用红。', '没有智能体时显示一行说明，不留空白面板。', 'snapshot 断开时脚注变红，行保留最后状态。'],
        ely: 'ActivityBar · Sidebar · lists::SelectableList · data_display::Badge',
      };
      if (v === 'plugins') return {
        flow: ['插件视图列出 daemon 注册表与本地声明；行内 badge 标来源。', '点击行在右侧打开 <b>480px 管理器抽屉</b>，主区域仍是终端。', '已停用的插件保留在列表，但动作不出现在面板；卸载走两步确认。'],
        states: ['悬停行只提亮；选中行强调条与抽屉同步。', '插件宿主只读时，行尾显示「只读」而不是禁用开关。', '日志与动作数来自 <b>plugin.list</b> 的按需读取，不轮询。'],
        ely: 'Sidebar · lists::SelectableList · Card · layout::Drawer',
      };
      return {
        flow: ['标题栏搜索或 <span class="mono">⌘⇧P</span> 打开命令面板；工作区药丸显示分支与连接状态。', 'Rail 切换 5 个侧栏视图；主区域始终是终端，管理面用抽屉/模态，不抢工作区。', '工作区树展开 tab 子行；点行聚焦，拖拽重排；数字键 <span class="mono">ctrl-b 1..9</span> 切换。', '<b>WORKTREES</b> 块列出聚焦仓库的工作树；点行打开/聚焦，<span class="mono">+</span> 创建。', '布局块预览 <span class="mono">.zenix/layout.toml</span>；点开 840px 检查器。'],
        states: ['选中行 2px 强调条；hover 只提亮，不改变布局。', '对话面板 &lt;1180 折叠成竖轨，侧栏 &lt;900 隐藏；折叠不写回 config。', '空工作区 / daemon 断开有独立空态（见 09）。', '侧栏脚注永远写明数据源：snapshot / event / plugin.list。'],
        ely: 'AppShell · TitleBar · ActivityBar · TabBar · StatusBar · Sidebar · SplitPane · Card',
      };
    },
  },
  {
    id: 'palette', num: '02', title: '命令面板', lede: '一个输入，三个来源：命令、工作区、插件动作。选中项、快捷键与来源徽章在同一行。',
    states: [{ id: 'open', label: '打开' }],
    render() {
      return shell({ rail: 'workspaces', sidebar: sidebarWorkspaces(), main: panesDefault(), aside: asideRail() }) + paletteOverlay() +
        pin(1, 392, 106, '输入即过滤；MRU 让最近用过的命令排在前面。') +
        pin(2, 392, 178, '三个命名 section：命令 / 工作区 / 插件动作。') +
        pin(3, 392, 240, '选中行：surface 填充 + 左侧 2px 强调条；Enter 执行。') +
        pin(4, 900, 336, '插件动作带来源徽章（daemon / local），与命令区分。') +
        pin(5, 392, 620, '底部常量：↑↓ / ↵ / esc，以及匹配与隐藏计数。');
    },
    notes() {
      return {
        flow: ['<span class="mono">⌘⇧P</span> 或标题栏搜索打开；输入 fuzzy 过滤；<span class="mono">↑↓</span> 选择，<span class="mono">Enter</span> 执行，<span class="mono">Esc</span> 关闭。', '命令行右侧显示目录里的真实 chord；插件动作显示声明里的 chord 或「palette」。', '工作区行显示标签数与当前标记；执行即聚焦。', '最多画 8 行；更多匹配在底部计数，滚动可见。'],
        states: ['没有匹配：显示一条空态 + 「新建工作区」兜底，不留死路。', '插件被停用：动作不出现在列表（<b>Catalog::runnable</b>），但插件仍可在侧栏看到。', '同一 chord 冲突时，目录优先；插件声明被丢弃，面板里也不展示。'],
        ely: 'navigation::palette::CommandPalette · forms::SearchInput · typography::Kbd',
      };
    },
  },
  {
    id: 'chat', num: '03', title: '对话面板', lede: '右侧 372px，四种状态：空态、对话、审批、历史。输入卡与思考块使用统一组件，文案全部走 i18n。',
    states: [{ id: 'empty', label: '空态' }, { id: 'conversation', label: '对话' }, { id: 'approval', label: '审批' }, { id: 'history', label: '历史' }],
    render(v) {
      const pins = v === 'empty'
        ? pin(1, 1420, 246, '空态只说“能做什么”，三个 starter 只填入输入框，不直接发送。') + pin(2, 1420, 322, 'Starter chips：填入后聚焦输入框，仍可修改。') + pin(3, 1420, 800, '输入卡：上下文行、两行输入、模型/语言选择器、圆形发送键。')
        : v === 'conversation'
          ? pin(1, 1420, 174, '思考块默认折叠；流式时显示“思考中”，完成后显示耗时。') + pin(2, 1420, 222, '工具卡：名称 / 状态 / 参数 / 结果；英文 Heading 全部本地化。') + pin(3, 1420, 470, '流式 markdown；代码块与表格跟随主题。') + pin(4, 1420, 760, '底部：token 数、复制/重试；流式时发送键变停止。')
          : v === 'approval'
            ? pin(1, 1420, 300, '审批卡用 warning 色，不用红色；默认焦点在「允许一次」。') + pin(2, 1420, 392, '三档：拒绝 / 总是允许（本次会话该工具）/ 允许一次。')
            : pin(1, 1420, 100, '历史行：标题 + 相对时间；当前行强调条。') + pin(2, 1420, 154, '重命名就地编辑；Enter 保存，Esc 取消。') + pin(3, 1420, 486, '删除两步：先确认条，再执行；不弹系统对话框。');
      return shell({ rail: 'workspaces', sidebar: sidebarWorkspaces(), main: panesDefault(), aside: asideChat(v) }) + pins;
    },
    notes(v) {
      if (v === 'empty') return {
        flow: ['Starter 只填入输入框，不直接发送；Enter 发送，Shift+Enter 换行。', '模型与推理强度来自 provider 目录。', '<span class="mono">/</span> 打开命令（新建 / 历史 / 清空）；<span class="mono">@</span> 提及留到下一版。'],
        states: ['没有 provider：空态提示先配置，并给一个「打开设置」入口。', '输入法：提交正常；行内 preedit 是 GPUI XIM 的上游限制，设计不额外造轮子。', '面板折叠时保留草稿与焦点，展开不丢。'],
        ely: 'ely_chat::{ChatContainer, MessageBubble, PromptInput, SuggestionChips} · forms::TextInput',
      };
      if (v === 'conversation') return {
        flow: ['流式 markdown 随事件重绘；滚动只在用户位于底部时跟随。', '思考块可展开；工具卡带参数/结果两个折叠区，耗时显示在头部。', '消息脚：token、复制、最后一条可重试；发送键在流式时变停止。'],
        states: ['工具运行中：卡片左缘 accent 呼吸；完成变绿，失败变红。', '错误：红色卡片 + 重试；重试会丢掉失败留下的尾部。', '压缩发生：在消息流末尾插一条 warning 说明，不打断对话。'],
        ely: 'StreamingMarkdown · MessageActions · ely_agent::ToolCallCard',
      };
      if (v === 'approval') return {
        flow: ['审批卡出现在工具调用之前；参数用 mono 展示。', '「允许一次」只跑这一次；「总是允许」写入本会话的集合；「拒绝」让模型收到拒绝结果。', '审批期间发送键变停止，可中断整轮。'],
        states: ['默认不聚焦危险按钮；Esc 等同拒绝。', '同一工具再次调用不再询问（本会话）。', '审批卡与工具卡不重复：批准后原地变成运行中卡片。'],
        ely: 'feedback::ConfirmationCard 的语义 + ely_chat 的卡片语言',
      };
      return {
        flow: ['历史按 updated_at 倒序；当前会话行有强调条。', '点击行加载会话；铅笔就地改名；垃圾桶进入两步删除。', '「新建」清空当前上下文并开新会话，旧会话保留。'],
        states: ['空历史：一行说明 + 新建按钮。', '改名失败：保留输入与错误提示，不吞掉。', '删除当前会话：先切到新会话，再删；避免空指针。'],
        ely: 'lists::SelectableList · forms::InlineEdit · feedback::ConfirmationCard',
      };
    },
  },
  {
    id: 'fleet', num: '04', title: '舰队驾驶舱', lede: '侧栏是花名册，右侧是卡片 + 状态转场 + TAIL 对比。终端仍在主区域，随时可以跳回去。',
    states: [{ id: 'roster', label: '花名册' }, { id: 'compare', label: 'TAIL 对比' }],
    render(v) {
      const compare = v === 'compare';
      return shell({ rail: 'fleet', sidebar: sidebarFleet(), main: panesDefault(), aside: asideFleet(compare) }) +
        pin(1, 1420, 112, '卡片 2×2：状态点、名字、角色、held age、窗格行、转场条。') +
        pin(2, 1420, 330, compare ? 'TAIL 对比：两列尾部各 200 行，差异行高亮。' : '转场条：每段一次 pane.agent_status_changed，颜色即状态。') +
        pin(3, 14, 170, 'Rail 舰队视图：侧栏花名册；点击行聚焦窗格。') +
        pin(4, 1420, 620, compare ? '只读尾部；完整日志在窗格或外部打开。' : '最近转场列表：时间、who、to。');
    },
    notes(v) {
      return {
        flow: ['状态来自 <b>pane.agent_status_changed</b> 推送；held age 由客户端本地计时。', '点卡片选中；眼睛聚焦窗格，终端图标打开 TAIL。', 'TAIL 对比选两个 agent，各取尾部 200 行；差异行高亮，不写回。', '侧栏花名册与 aside 卡片共享同一份舰队数据。'],
        states: ['working / success、waiting / warning、done / idle、error / danger；不使用红色表示等待。', '没有选中对比对象时，TAIL 页显示选择提示，不是空白。', 'daemon 断开：卡片保留最后状态，角标显示陈旧时间。'],
        ely: 'ely_dashboard::Card · data_display::{Badge, Timeline} · layout::SplitPane',
      };
    },
  },
  {
    id: 'plugins', num: '05', title: '插件', lede: '列表在侧栏，管理器是 480px 抽屉；终端不被接管。安装合同、argv 风险、动作与日志都在一处。',
    states: [{ id: 'list', label: '列表' }, { id: 'manager', label: '管理器' }, { id: 'trust', label: '信任审查' }],
    render(v) {
      if (v === 'list') {
        return shell({ rail: 'plugins', sidebar: sidebarPlugins(), main: panesDefault(), aside: asideRail() }) +
          pin(1, 250, 150, '插件列表：名称、版本、来源 badge、动作数；停用项降低不透明度。') +
          pin(2, 720, 300, '主区域仍是终端；选插件不会把工作区顶掉。') +
          pin(3, 250, 250, '点击行打开右侧管理器抽屉。');
      }
      if (v === 'trust') {
        return shell({ rail: 'plugins', sidebar: sidebarPlugins(), main: panesDefault(), aside: asideRail() }) + trustModal() +
          pin(1, 360, 90, '信任审查：链接前逐条列出 argv 与风险等级。') +
          pin(2, 1070, 420, '勾选后才允许「信任并链接」。') +
          pin(3, 1070, 90, '路径按 herdr v0.9.5 的算法计算，和插件运行时看到的一致。');
      }
      return shell({ rail: 'plugins', sidebar: sidebarPlugins(), main: panesDefault(), aside: asideRail() }) + pluginDrawer() +
        pin(1, 1420, 88, '管理器抽屉 480px：概览 / 动作 / 面板 / 日志。') +
        pin(2, 1420, 166, '合同：manifest、state、config、refresh；路径是插件真实拿到的。') +
        pin(3, 1420, 300, 'argv 逐条分级：低 / 中 / 插件内；不是一句“已信任”。') +
        pin(4, 1420, 700, '启用开关与卸载在底部；卸载走共享确认。');
    },
    notes(v) {
      if (v === 'list') return {
        flow: ['侧栏列出 daemon 注册表与 local 声明；行内 badge 标来源。', '点击行打开抽屉；停用插件仍可见，但动作不出现。', '列表底部说明 <b>plugin.list</b> 按需读取，不轮询。'],
        states: ['无插件：一行说明 + 「打开插件目录」。', '只读插件：显示「只读」而不是禁用开关。', '插件被删除但仍在注册表：状态点变灰，详情给「重新读取」。'],
        ely: 'lists::SelectableList · Badge · Sidebar',
      };
      if (v === 'trust') return {
        flow: ['链接前读取每个 argv：低风险/中风险/插件内脚本；中风险默认展开。', '必须勾选“已阅读”；取消则不入注册表。', '链接后仍可停用或卸载。'],
        states: ['高风险管理动作不可勾选绕过；只能取消。', '路径不可解析时显示原始字符串，不隐藏。', '审查页只读；改声明要回到文件。'],
        ely: 'feedback::ConfirmationCard · Card · forms::Checkbox',
      };
      return {
        flow: ['概览：合同、命令与风险、声明的 UI、日志。', '动作页：每个 action 一行，显示 chord 与运行状态；面板页：声明的 pane/segment。', '启用/停用是 <b>plugin.enable/disable</b>；卸载是 <b>plugin.unlink</b>，先确认。'],
        states: ['动作运行中：行内禁用重复点击，日志实时追加。', '插件只读：动作可运行但不可卸载。', '抽屉宽度 480，不遮住整块终端；Esc 关闭。'],
        ely: 'layout::Drawer · Card · tables · feedback::ConfirmationCard',
      };
    },
  },
  {
    id: 'events', num: '06', title: '事件', lede: '侧栏是流，点击一行在右侧 420px 检查器里看 JSON。事件契约单独一页，21 个名字两种拼写都在。',
    states: [{ id: 'stream', label: '事件流' }, { id: 'contract', label: '契约' }],
    render(v) {
      const base = shell({ rail: 'events', sidebar: sidebarEvents(), main: panesDefault(), aside: asideRail() });
      if (v === 'contract') {
        return base + contractModal() +
          pin(1, 380, 90, '契约弹层：21 个 SUBSCRIBED 事件，wire 与模型两种拼写。') +
          pin(2, 380, 420, 'JSON lane 只用于 plugin.* / worktree.* / layout.* 的按需读取。');
      }
      return base + eventDrawer() +
        pin(1, 250, 212, '侧栏事件流：时间、事件名、对象；行高 40px。') +
        pin(2, 1005, 120, '检查器 420px：JSON / 原始 / 契约；seq 与延迟在头部。') +
        pin(3, 1005, 300, '元数据：来源、类别、延迟。') +
        pin(4, 250, 818, '脚注的「契约」打开 21 事件清单。');
    },
    notes(v) {
      return v === 'contract' ? {
        flow: ['契约页列出 21 个 SUBSCRIBED 事件名，dotted 与 wire（snake_case）并排。', '脚注说明 JSON lane 的边界：plugin.* / worktree.* / layout.* 是按需读取，不是推送。', '复制清单给插件作者。'],
        states: ['事件名以 daemon 版本为准；升级 daemon 时这页是核对入口。', '未知事件：流里显示原始 name，不 panic。'],
        ely: 'overlays::Dialog · typography::Code · tables',
      } : {
        flow: ['侧栏订阅一条流；过滤芯片按类别筛选，暂停只停绘制不取消订阅。', '点击行打开检查器；JSON 可复制，原始 payload 保留字节。', '类别颜色：智能体 / 插件 / 工作区 / 布局；错误用 danger。'],
        states: ['暂停：流继续进环形缓冲，恢复后补画；不丢事件。', '断开：脚注变红，恢复后从最后 seq 继续。', '事件过多：只保留最近 500 条，计数器提示丢弃数。'],
        ely: 'lists::SelectableList · data_display::Badge · typography::Code · layout::Drawer',
      };
    },
  },
  {
    id: 'layout', num: '07', title: '布局与工作树', lede: '布局是随仓库提交的文件，用 840px 模态看代码与 split 树；工作树的新建和移除都在原地确认。',
    states: [{ id: 'layout', label: '布局' }, { id: 'new', label: '新建工作树' }, { id: 'remove', label: '移除确认' }],
    render(v) {
      const base = shell({ rail: 'workspaces', sidebar: sidebarWorkspaces(), main: panesDefault(), aside: asideRail() });
      if (v === 'new') return base + newWorktreeModal() +
        pin(1, 490, 90, '新建：分支名、基准分支、将创建的路径（daemon 命名）。') +
        pin(2, 930, 330, '创建后出现在 WORKTREES 块；不轮询，靠事件刷新。');
      if (v === 'remove') return base + removeWorktreeModal() +
        pin(1, 490, 90, '移除确认：先说明脏树；普通移除会被 daemon 拒绝。') +
        pin(2, 500, 300, '强制移除是显式开关，默认关闭；按钮是 danger。');
      return base + layoutModal() +
        pin(1, 290, 72, '头部：文件路径、同步状态、复制/导出/应用。') +
        pin(2, 290, 140, '左侧是真实文件文本：TOML、稳定排序、不写 pane_id。') +
        pin(3, 1150, 150, '右侧 split 树按 ratio 画；叶子显示 label。') +
        pin(4, 1150, 420, '应用不是幂等：新建标签，不替换现有窗格。');
    },
    notes(v) {
      if (v === 'new') return {
        flow: ['从聚焦工作区的仓库创建；分支名与路径由 daemon 命名法生成。', '创建成功发 <b>worktree.created</b>，侧栏块即时插入。', '失败保留输入并显示 daemon 原文。'],
        states: ['分支已存在：输入框下方提示，不直接提交。', '路径冲突：显示将使用的后缀（~2、~3）。', '没有聚焦仓库：入口禁用并说明原因。'],
        ely: 'layout::Dialog · forms::{TextInput, Select} · feedback::ConfirmationCard',
      };
      if (v === 'remove') return {
        flow: ['移除前读取工作树状态；脏树要显式打开强制。', '普通移除失败时，把 daemon 的拒绝原因原样展示。', '移除成功发 <b>worktree.removed</b>，块即时消失。'],
        states: ['默认焦点在取消；Esc 关闭。', '强制开关打开后，确认按钮才可用。', '不可逆操作有第二行说明，不藏在 tooltip。'],
        ely: 'feedback::ConfirmationCard · forms::Switch',
      };
      return {
        flow: ['布局块显示文件摘要；点开模态看完整文本与 split 树。', '应用走 <b>layout.apply</b>：总是新建标签，不幂等；按钮文案写明。', '导出读 <b>layout.export</b> 并写文件；复制路径给 CLI。'],
        states: ['文件缺失：显示空态 + 「从当前标签导出」。', '解析失败：保留原文，错误指向行号；不覆盖文件。', 'schema 版本不认识：只读展示，禁用应用。'],
        ely: 'layout::Dialog · Card · typography::Code · SplitPane',
      };
    },
  },
  {
    id: 'settings', num: '08', title: '设置', lede: '1080×780 的居中对话框；左侧页导航，右侧是卡片分组。写入即生效，卡片是唯一的设置容器。',
    states: [
      { id: 'appearance', label: '外观' }, { id: 'terminal', label: '终端' }, { id: 'ai', label: 'AI' },
      { id: 'keys', label: '按键' }, { id: 'plugins', label: '插件' }, { id: 'mcp', label: 'MCP' },
      { id: 'about', label: '关于' },
    ],
    render(v) {
      return shell({ rail: 'workspaces', sidebar: sidebarWorkspaces(), main: panesDefault(), aside: asideRail() }) + settingsDialog(v) +
        pin(1, 170, 86, '标题栏：设置名与 config 路径；关闭在右上。') +
        pin(2, 170, 146, '页导航 196px：搜索、分组、选中项强调条。') +
        pin(3, 1270, 118, '页头：标题、一句说明、恢复本页默认。') +
        pin(4, 1270, 176, '卡片：标题 / 说明 / 设置行；行高 38px，发丝线分隔。') +
        pin(5, 1270, 250, '控件：Segmented / Select / Switch；改动立即写入。') +
        pin(6, 190, 740, '页脚：保存状态、路径、关闭；不藏「保存」按钮。');
    },
    notes(v) {
      if (v === 'keys') return {
        flow: ['命令目录是唯一事实源；这页只渲染目录里的 chord，不另存一份。', '搜索命令；点击 chord 可录制，冲突时提示被谁占用。', '前缀键可改；bare-key 插件动作排在 prefix 之后。'],
        states: ['冲突：行内 warning，保存前不生效。', '保留键不可覆盖；显示锁标记。', '恢复本页默认只重置本页。'],
        ely: 'ely_settings::KeyboardShortcutsList · forms::HotkeyInput',
      };
      if (v === 'mcp') return {
        flow: ['编排服务与反向桥分开陈述；状态来自运行时，不是配置。', '工具清单只读；插件工具数随注册表变化。', '复制 socket / 命令给外部客户端。'],
        states: ['服务未启动：状态徽章变灰，给「查看日志」。', '插件工具为 0：说明「当前没有已启用的插件 action」。', '不提供无法执行的开关。'],
        ely: 'SettingsSection · SettingsRow · typography::Code',
      };
      if (v === 'plugins') return {
        flow: ['宿主事实 + 插件声明的设置页；插件设置是只读投影。', '链接前审查默认开启；卸载两步确认。', '设置变更写进插件自己的 config，不写 zenix config。'],
        states: ['插件停用：其设置页隐藏，但数据保留。', '插件设置页损坏：显示解析错误，不阻塞其它页。', '只读页没有保存按钮。'],
        ely: 'SettingsSection · Card · forms::Switch',
      };
      return {
        flow: ['外观 / AI 设置分卡片：每张卡只回答一类问题。', '改动立即写入 config.json（config::update），没有全局保存按钮。', '恢复本页默认只重置本页字段；页脚显示保存状态与路径。', '搜索设置跨页命中，进入时高亮对应行。'],
        states: ['写盘失败：页脚变红并保留内存值，下一次改动重试。', '数值越界：就地提示并夹紧，不静默。', '插件/MCP 只读事实不提供假开关。'],
        ely: 'ely_settings::{SettingsLayout, SettingsSection, SettingsRow, ThemeSelector, AccentColorPicker, SyntaxThemePicker, DensitySelector, ThemeEditor, ThemeImporter} · layout::Card',
      };
    },
  },
  {
    id: 'empty', num: '09', title: '空态与首次运行', lede: '没有工作区、没有 daemon 时的第一屏。四个入口 + 一条明确的启动路径，不留死路。',
    states: [{ id: 'first', label: '首次运行' }],
    render() {
      return shell({
        workspace: '无工作区', noWorkspace: true, offline: true, searchHint: '搜索命令（daemon 未连接）',
        rail: 'workspaces',
        sidebar: `<aside class="sidebar">
          ${sidebarHead('工作区', '0')}
          <div class="sidebar-body" style="display:flex;flex-direction:column;align-items:center;justify-content:center;gap:8px;text-align:center">
            ${icon('folder', 'xl')}
            <div style="font-size:12px;font-weight:600">还没有工作区</div>
            <div class="mini" style="font-size:10.5px">新建后会出现在这里</div>
            <span class="btn sm primary">${icon('plus', 'xs')} 新建工作区</span>
          </div>
          <div class="sidebar-foot">${dot('err')}<span>等待 daemon</span></div>
        </aside>`,
        tabs: null,
        main: emptyMain(),
        status: '<div class="statusbar"><div class="group"><span class="dot err"></span><span>未连接</span><span class="dim">·</span><span>没有工作区</span></div><div class="right"><span class="seg"><span class="kbd">ctrl</span><span class="kbd">b</span> 前缀</span><span class="seg dim">snapshot 等待中</span></div></div>',
        aside: asideRail(),
      }) +
        pin(1, 720, 300, '第一屏先说清楚：没有工作区，也没有 daemon。') +
        pin(2, 720, 540, '四个入口：新建、打开布局、浏览插件、启动 daemon。') +
        pin(3, 720, 640, 'daemon 卡片：解释播种路径，给日志和启动两个动作。') +
        pin(4, 250, 80, '侧栏空态也有新建入口，不是空白。');
    },
    notes() {
      return {
        flow: ['首次运行：客户端把 <b>res/herdr</b> 播种到 <b>~/.local/bin/herdr</b>，再启动 daemon。', '四个入口都可达；「启动 daemon」直接在原地跑，不打开终端。', '新建工作区是主按钮；打开布局走文件选择器。'],
        states: ['daemon 启动失败：卡片展开日志尾部，给复制命令。', '没有 git 仓库：新建工作区仍可用，只是没有 worktree 块。', '离线时状态栏明确，不显示假连接。'],
        ely: 'feedback::EmptyState · Card · Button · typography::Kbd',
      };
    },
  },
];

/* ---------- boot ---------- */

const STATE = {};

function screenById(id) {
  return SCREENS.find((s) => s.id === id) || SCREENS[0];
}

function renderNavScreens() {
  const host = document.getElementById('nav-screens');
  if (!host) return;
  host.innerHTML = SCREENS.map((s) => `<a href="#s-${s.id}" data-target="s-${s.id}"><b>${s.num}</b>${s.title}</a>`).join('');
}

function notesHtml(n) {
  return `<div class="note-box"><h5>主流程</h5><ul>${n.flow.map((x) => `<li>${x}</li>`).join('')}</ul><div class="elyline"><b>组件</b> ${n.ely}</div></div>
    <div class="note-box"><h5>状态与边界</h5><ul>${n.states.map((x) => `<li>${x}</li>`).join('')}</ul></div>`;
}

function currentState(s) {
  return STATE[s.id] || s.states[0].id;
}

function renderScreen(s) {
  const board = document.getElementById(`board-${s.id}`);
  const notes = document.getElementById(`notes-${s.id}`);
  const tabs = document.querySelector(`.state-tabs[data-screen="${s.id}"]`);
  if (!board) return;
  const st = currentState(s);
  board.innerHTML = s.render(st);
  notes.innerHTML = notesHtml(s.notes(st));
  if (tabs) {
    tabs.querySelectorAll('button').forEach((b) => b.classList.toggle('on', b.dataset.state === st));
  }
  scaleBoards();
}

function renderScreens() {
  const host = document.getElementById('screens');
  if (!host) return;
  host.innerHTML = SCREENS.map((s) => `<section class="screen" id="s-${s.id}">
    <div class="screen-head">
      <div class="screen-title"><span class="eyebrow">${s.num} · SCREEN</span><h2>${s.title}</h2><p>${s.lede}</p></div>
      <div class="state-tabs" data-screen="${s.id}">${s.states.map((st, i) => `<button data-screen="${s.id}" data-state="${st.id}" class="${i === 0 ? 'on' : ''}">${st.label}</button>`).join('')}</div>
    </div>
    <div class="board-frame">
      <div class="board-toolbar"><span class="traffic"><i></i><i></i><i></i></span><span class="path">zenix / ${s.id}</span><span class="right"><span>1440 × 900</span><span class="zoom">fit</span></span></div>
      <div class="board-scale" data-screen="${s.id}"><div class="board" id="board-${s.id}"></div></div>
    </div>
    <div class="notes" id="notes-${s.id}"></div>
  </section>`).join('');
  SCREENS.forEach(renderScreen);
}

function scaleBoards() {
  document.querySelectorAll('.board-scale').forEach((wrap) => {
    const w = wrap.clientWidth;
    if (!w) return;
    const s = Math.min(1, w / 1440);
    const board = wrap.querySelector('.board');
    if (board) board.style.transform = `scale(${s})`;
    wrap.style.height = `${900 * s}px`;
  });
}

function setupStyle() {
  const params = new URLSearchParams(location.search);
  const initial = params.get('style') || 'dark';
  document.documentElement.dataset.style = initial;
  const sync = () => {
    const style = document.documentElement.dataset.style;
    document.querySelectorAll('.style-switch button, .style-card').forEach((b) => {
      b.classList.toggle('on', b.dataset.style === style);
    });
  };
  document.querySelectorAll('.style-switch button, .style-card').forEach((b) => {
    b.addEventListener('click', () => {
      document.documentElement.dataset.style = b.dataset.style;
      sync();
    });
  });
  sync();
}

function setupTabs() {
  document.addEventListener('click', (e) => {
    const btn = e.target.closest('[data-state]');
    if (btn && btn.dataset.screen) {
      STATE[btn.dataset.screen] = btn.dataset.state;
      renderScreen(screenById(btn.dataset.screen));
    }
  });
}

function setupScrollSpy() {
  const links = [...document.querySelectorAll('.nav a')];
  const targets = links.map((a) => document.getElementById(a.dataset.target)).filter(Boolean);
  const io = new IntersectionObserver((entries) => {
    entries.forEach((entry) => {
      if (!entry.isIntersecting) return;
      links.forEach((a) => a.classList.toggle('on', a.dataset.target === entry.target.id));
    });
  }, { rootMargin: '-15% 0px -70% 0px', threshold: 0 });
  targets.forEach((t) => io.observe(t));
}

function boot() {
  const params = new URLSearchParams(location.search);
  const style = params.get('style') || 'dark';
  document.documentElement.dataset.style = style;
  if (params.get('embed') === '1') {
    document.body.classList.add('embed');
    const s = screenById(params.get('screen'));
    const st = params.get('state') || s.states[0].id;
    document.body.innerHTML = `<div class="board" id="embed-board"></div>`;
    document.getElementById('embed-board').innerHTML = s.render(st);
    if (params.get('pins') === '0') document.querySelectorAll('.pin').forEach((p) => p.remove());
    return;
  }
  renderNavScreens();
  renderScreens();
  setupStyle();
  setupTabs();
  setupScrollSpy();
  window.addEventListener('resize', scaleBoards);
  setTimeout(scaleBoards, 30);
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', boot);
} else {
  boot();
}
