'use strict';
(() => {
  const $ = id => document.getElementById(id);
  const invoke = (command, args = {}) => AppIPC.invoke(command, args);
  let selected = 'codex-app-coord', agents = [], messages = [], mode = 'tree', hasMore = false;
  let pendingSend = null, sending = false, refreshing = false, historyVersion = 0, historyPages = 1;
  const collapsed = new Set();
  const node = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text !== undefined) e.textContent = text; return e; };
  const notice = text => { $('notice').textContent = text; };
  const agentById = id => agents.find(a => a.id === id);
  const matches = a => (! $('provider').value || a.provider === $('provider').value) && (! $('project-filter').value || a.role === 'intake' || a.project === $('project-filter').value) && (! $('status-filter').value || a.availability === $('status-filter').value) && `${a.name} ${a.project || ''} ${a.purpose} ${a.provider}`.toLowerCase().includes($('search').value.toLowerCase());
  function ancestry(agent) {
    const chain = []; let current = agent;
    while (current && chain.length < 4) { chain.unshift(current); current = agentById(current.parent_id); }
    return chain;
  }
  function row(a, depth) {
    const b = node('button', `agent-row depth-${depth}${a.id === selected ? ' selected' : ''}`);
    b.type = 'button'; b.dataset.agent = a.id; b.setAttribute('aria-pressed', String(a.id === selected));
    b.title = `${a.provider} / ${a.role} / ${a.availability} / simulated`;
    b.append(node('span','agent-icon',a.role === 'intake' ? '◇' : a.role === 'coordinator' ? '▱' : a.purpose === 'refuter' ? '✓' : '↳'));
    const label = node('span','agent-label'); label.append(node('strong','',a.name),node('small','',`${a.role === 'subagent' ? a.purpose : a.role} · simulated`)); b.append(label);
    b.append(node('span',`state-dot ${a.availability}`));
    if (a.queued) b.append(node('span','queue-badge',String(a.queued)));
    b.onclick = () => select(a.id); return b;
  }
  function renderRoster() {
    const visible = new Set();
    agents.filter(matches).forEach(a => ancestry(a).forEach(p => visible.add(p.id)));
    $('agent-total').textContent = `${visible.size} of ${agents.length}`;
    const roster = $('roster'); roster.replaceChildren();
    if (!visible.size) { roster.append(node('p','no-results','No agents match these filters.')); return; }
    const group = (title,key) => {
      const details = node('details','provider-group'); details.open = !collapsed.has(key);
      details.append(node('summary','',title)); details.ontoggle = () => { if (details.open) collapsed.delete(key); else collapsed.add(key); }; roster.append(details); return details;
    };
    if (mode === 'tree') {
      for (const provider of ['claude','codex']) {
        const root = agents.find(a => a.provider === provider && a.role === 'intake');
        if (!root || !visible.has(root.id)) continue;
        const g = group(`${provider === 'claude' ? 'CLAUDE' : 'CODEX'} · SIMULATED`,provider);
        g.append(row(root,0));
        for (const c of agents.filter(a => a.parent_id === root.id && visible.has(a.id))) {
          g.append(row(c,1)); agents.filter(a => a.parent_id === c.id && visible.has(a.id)).forEach(a => g.append(row(a,2)));
        }
      }
    } else {
      const intake = group('INTAKE · WORKSPACE','intake');
      agents.filter(a => a.role === 'intake' && visible.has(a.id)).forEach(a => intake.append(row(a,0)));
      const projects = [...new Set(agents.filter(a => visible.has(a.id) && a.project).map(a => a.project))];
      for (const project of projects) {
        const g = group(project.toUpperCase(),project);
        for (const c of agents.filter(a => a.project === project && a.role === 'coordinator' && visible.has(a.id))) {
          g.append(node('p','project-group-title',`${c.provider === 'claude' ? 'Claude' : 'Codex'} / ${agentById(c.parent_id).name}`));
          g.append(row(c,0)); agents.filter(a => a.parent_id === c.id && visible.has(a.id)).forEach(a => g.append(row(a,1)));
        }
      }
    }
  }
  function renderDetails() {
    const a = agentById(selected); if (!a) return;
    $('agent-name').textContent = a.name; $('detail-name').textContent = a.name;
    $('agent-subtitle').textContent = `${a.provider === 'claude' ? 'Claude' : 'Codex'} · ${a.project || 'Workspace intake'}`;
    $('agent-avatar').textContent = $('detail-avatar').textContent = a.provider === 'claude' ? 'C' : 'X';
    $('availability').textContent = a.availability; $('availability').className = `availability ${a.availability}`;
    $('breadcrumb').textContent = ancestry(a).map(p => p.name).join(' › ');
    $('details').replaceChildren();
    for (const [label,value] of [['Provider',a.provider === 'claude' ? 'Claude' : 'Codex'],['Role',a.role],['Reports to',a.parent_id ? agentById(a.parent_id).name : 'Workspace'],['Project',a.project || 'All projects'],['Assignment',a.purpose],['Queued',String(a.queued)]]) $('details').append(node('dt','',label),node('dd','',value));
    document.querySelectorAll('[data-state]').forEach(b => b.setAttribute('aria-pressed',String(b.dataset.state === a.availability)));
    for (const [id,role] of [['intake-count','intake'],['coord-count','coordinator'],['sub-count','subagent']]) $(id).textContent = agents.filter(a => a.role === role).length;
    $('queue-count').textContent = agents.reduce((n,a) => n + a.queued,0); $('nav-count').textContent = agents.length;

  }
  function renderMessages(scroll) {
    const container = $('messages'); container.replaceChildren(); $('load-older').hidden = !hasMore;
    if (!messages.length) {
      const empty = node('div','empty-state'); empty.append(node('div','empty-icon','↗'),node('h3','','Start a conversation'),node('p','','Send a message to this simulated agent. Your conversation stays here when you restart the app.')); container.append(empty);
    }
    for (const m of messages) {
      const article = node('article',`message ${m.sender}`); article.dataset.messageId = m.id;
      const head = node('div','message-head'); head.append(node('strong','',m.sender === 'user' ? 'You' : agentById(selected).name),node('span','',m.sender === 'user' ? '' : '· simulated'));
      const time = node('time','',m.created_at.slice(11,16)); time.dateTime = m.created_at; time.title = m.created_at; head.append(time);
      article.append(head,node('div','message-body',m.body),node('div',`message-state ${m.state}`,m.state === 'queued' ? '◷ Saved · queued until agent is idle' : m.sender === 'user' ? '✓ Delivered locally' : '✓ Simulated reply saved')); container.append(article);
    }
    if (scroll) $('history').scrollTop = $('history').scrollHeight;
  }
  async function refresh() {
    if (refreshing) return; refreshing = true; const id = selected, version = historyVersion;
    try {
      const s = await invoke('workspace_snapshot',{agentId:id});
      let page = s;
      // Refresh every loaded page: an older queued message may have been delivered
      // while the newest page is filled by newer requests. Bound work/DOM to 1000.
      for (let i=1;i<historyPages && page.has_more;i++) {
        page = await invoke('workspace_snapshot',{agentId:id,before:page.messages[0].id});
        s.messages = [...page.messages,...s.messages];
      }
      if (id !== selected || version !== historyVersion) return;
      const rosterChanged = JSON.stringify(agents) !== JSON.stringify(s.agents);
      agents = s.agents;
      const changed = JSON.stringify(messages) !== JSON.stringify(s.messages);
      messages = s.messages; hasMore = page.has_more && historyPages < 10;
      if (rosterChanged) { renderRoster(); renderDetails(); }
      if (changed || !messages.length) renderMessages(changed);
      document.body.dataset.ready = 'true';
    } catch (e) { notice(`Unable to load workspace: ${String(e)}`); }
    finally { refreshing = false; }
  }
  async function select(id) {
    if (id === selected) return; selected = id; historyVersion++; historyPages = 1; messages = []; hasMore = false; pendingSend = null; $('message').value = ''; notice('');
    renderRoster(); renderDetails(); renderMessages(false);
    // An in-flight old snapshot is discarded; a follow-up load uses the new recipient.
    if (!refreshing) await refresh();
  }
  $('composer').onsubmit = async event => {
    event.preventDefault(); if (sending) return; const body = $('message').value;
    if (!body.trim()) return;
    const recipient = selected;
    if (!pendingSend || pendingSend.agentId !== recipient || pendingSend.body !== body) pendingSend = {agentId:recipient,body,requestId:crypto.randomUUID()};
    const request = pendingSend; sending = true; $('send').disabled = true;
    try {
      await invoke('workspace_send',request);
      if (selected === recipient && $('message').value === body) $('message').value = '';
      if (pendingSend === request) pendingSend = null;
      notice('Message saved locally.'); await refresh();
    } catch (e) { notice(`Message not confirmed. Retry keeps the same request: ${String(e)}`); }
    finally { sending = false; $('send').disabled = false; }
  };
  document.querySelectorAll('[data-state]').forEach(b => b.onclick = async () => {
    const id = selected; try { await invoke('workspace_control',{agentId:id,availability:b.dataset.state}); notice('Availability saved.'); await refresh(); } catch(e) { notice(String(e)); }
  });
  $('load-older').onclick = async () => {
    if (!messages.length || historyPages >= 10) return;
    historyPages++; historyVersion++; $('load-older').disabled = true;
    try {
      await refresh();
      if (historyPages === 10) notice('Showing up to 1000 recent messages.');
    } catch(e) { notice(String(e)); } finally { $('load-older').disabled = false; }
  };
  for (const id of ['search','provider','status-filter','project-filter']) $(id).addEventListener('input',()=>{renderRoster();if(id!=='search')AppPrefs.remember({hierarchy_provider:$('provider').value,hierarchy_status:$('status-filter').value,hierarchy_project:$('project-filter').value});});
  function setMode(next) { mode = next; $('tree-view').setAttribute('aria-pressed',String(mode === 'tree')); $('project-view').setAttribute('aria-pressed',String(mode === 'project')); renderRoster(); }
  $('tree-view').onclick = () => {setMode('tree');AppPrefs.remember({hierarchy_view:'tree'});}; $('project-view').onclick = () => {setMode('project');AppPrefs.remember({hierarchy_view:'project'});};
  let preferencesInitialized=false;
  window.addEventListener('agentview:preferences',e=>{if(preferencesInitialized)return;preferencesInitialized=true;const p=e.detail;if(p.remember_filters){$('provider').value=p.hierarchy_provider;$('status-filter').value=p.hierarchy_status;$('project-filter').value=p.hierarchy_project;setMode(p.hierarchy_view);}renderRoster();});
  $('open-docs').onclick = async () => { try { await invoke('workspace_open_docs'); } catch(e) { notice(String(e)); } };
  let ticking = false;
  async function pulse() { if (ticking||document.body.dataset.demo!=='true') return; ticking = true; try { await invoke('workspace_tick'); await refresh(); } catch(e) { notice(String(e)); } finally { ticking = false; } }
  refresh(); setInterval(pulse,1000);
})();
