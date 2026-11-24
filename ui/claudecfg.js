'use strict';
(() => {
  const $=id=>document.getElementById(id);let view=null;
  const cell=(tr,t,cls)=>{const td=document.createElement('td');td.textContent=t;if(cls)td.className=cls;tr.append(td);return td;};
  const mcpText=l=>l.length?l.map(s=>`${s.name} (${s.kind}${s.target?' · '+s.target:''}${s.has_env?' · has secrets, hidden':''})`).join(', '):'none';
  function draw(){
    const body=$('cc-body');body.replaceChildren();if(!view)return;
    const q=$('cc-filter').value.trim().toLowerCase();
    for(const p of view.projects.filter(p=>!q||p.path.toLowerCase().includes(q))){
      const tr=document.createElement('tr');cell(tr,p.path,'mono');
      cell(tr,p.trusted===null?'—':p.trusted?'Yes':'No');cell(tr,String(p.allowed_tools));cell(tr,String(p.history_entries));cell(tr,mcpText(p.mcp));
      if(!p.exists)tr.title='Folder no longer exists';body.append(tr);
    }
  }
  async function load(){
    try{view=await AppIPC.invoke('claude_config_view');}catch(e){$('cc-note').textContent=String(e);return;}
    $('cc-note').textContent=view.found?`${view.note} File size ${Math.round(view.size/1024)} KB, ${view.top_level_keys} top-level entries${view.has_account_data?', includes account data (not shown)':''}.`:'No ~/.claude.json found. Claude Code creates it on first use.';
    $('cc-projects').textContent=String(view.projects.length);$('cc-global').textContent=String(view.global_mcp.length);
    $('cc-untrusted').textContent=String(view.projects.filter(p=>p.trusted===false).length);$('cc-missing').textContent=String(view.projects.filter(p=>!p.exists).length);
    $('cc-mcp').textContent=mcpText(view.global_mcp);draw();
  }
  $('cc-filter').oninput=draw;$('cc-reload').onclick=load;$('show-cc').addEventListener('click',load);
})();
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  const cell=(tr,t,cls)=>{const td=document.createElement('td');td.textContent=t;if(cls)td.className=cls;tr.append(td);};
  let inv=[];
  function tab(name){
    for(const b of document.querySelectorAll('#cc-tabs [data-cc-tab]'))b.setAttribute('aria-selected',String(b.dataset.ccTab===name));
    for(const n of ['projects','settings','skills','memory','history'])$('cc-pane-'+n).hidden=n!==name;
    if(name==='settings')loadSettings();if(name==='skills')loadInv();if(name==='memory')loadMem();if(name==='history')loadHist();
  }
  async function loadSettings(){
    let v;try{v=await invoke('claude_settings_view');}catch(e){$('cs-note').textContent=String(e);return;}
    $('cs-note').textContent=v.note;
    $('cs-allow').textContent=String(v.allow.length);$('cs-deny').textContent=String(v.deny.length);$('cs-ask').textContent=String(v.ask.length);$('cs-hooks').textContent=String(v.hooks.length);
    $('cs-mode').textContent=(v.default_mode?`Default permission mode: ${v.default_mode}. `:'')+v.files.map(f=>`${f.name}: ${!f.found?'not found':f.valid?'read':'not valid JSON'}`).join(' · ');
    const rules=$('cs-rules');rules.replaceChildren();
    for(const [kind,list] of [['Deny',v.deny],['Ask',v.ask],['Allow',v.allow]])for(const r of list){const tr=document.createElement('tr');cell(tr,kind);cell(tr,r.text,'mono');cell(tr,r.source);rules.append(tr);}
    const hooks=$('cs-hook-rows');hooks.replaceChildren();
    for(const h of v.hooks){const tr=document.createElement('tr');cell(tr,h.event);cell(tr,h.matcher||'any');cell(tr,h.kind);cell(tr,h.program||'—','mono');cell(tr,h.source);hooks.append(tr);}
    const other=[];if(v.model)other.push(`Model: ${v.model}`);if(v.status_line)other.push(`Status line program: ${v.status_line}`);if(v.helpers_set.length)other.push(`Helper commands set (not shown): ${v.helpers_set.join(', ')}`);
    other.push(v.env_names.length?`Environment variable names (values never shown): ${v.env_names.join(', ')}`:'No environment variables set in these files');if(v.plugins.length)other.push(`Enabled plugins: ${v.plugins.join(', ')}`);
    $('cs-other').textContent=other.join(' · ');
  }
  function drawInv(){
    const q=$('ci-filter').value.trim().toLowerCase();const body=$('ci-rows');body.replaceChildren();
    for(const e of inv.filter(e=>!q||(e.name+' '+e.description).toLowerCase().includes(q))){const tr=document.createElement('tr');cell(tr,e.kind);cell(tr,e.name,'mono');cell(tr,e.description);cell(tr,e.extra);body.append(tr);}
  }
  async function loadInv(){try{inv=await invoke('claude_inventory');}catch{inv=[];}drawInv();}
  let mem=[],sess=[];
  async function showMem(){
    const k=$('cm-select').value;if(!k){$('cm-text').textContent='';$('cm-meta').textContent='No memory or instruction files found.';return;}
    try{const f=await invoke('claude_memory_read',{key:k});$('cm-text').textContent=f.text;$('cm-meta').textContent=f.truncated?'Showing the first 64 KB.':'';}catch(e){$('cm-text').textContent='';$('cm-meta').textContent=String(e);}
  }
  async function loadMem(){
    try{mem=await invoke('claude_memory_list');}catch{mem=[];}
    const sel=$('cm-select');sel.replaceChildren();
    for(const e of mem){const o=document.createElement('option');o.value=e.key;o.textContent=`${e.group} / ${e.name}`;sel.append(o);}
    showMem();
  }
  function drawHist(){
    const q=$('ch-filter').value.trim().toLowerCase();const body=$('ch-rows');body.replaceChildren();
    for(const s of sess.filter(s=>!q||(s.project+' '+s.id).toLowerCase().includes(q)).slice(0,200)){
      const tr=document.createElement('tr');cell(tr,new Date(s.modified*1000).toLocaleString());cell(tr,s.project);cell(tr,s.size>=1048576?`${(s.size/1048576).toFixed(1)} MB`:`${Math.max(1,Math.round(s.size/1024))} KB`);cell(tr,s.resume,'mono');body.append(tr);
    }
  }
  async function loadHist(){try{sess=await invoke('claude_sessions_list');}catch{sess=[];}drawHist();}
  $('cm-select').onchange=showMem;$('ch-filter').oninput=drawHist;
  for(const b of document.querySelectorAll('#cc-tabs [data-cc-tab]'))b.onclick=()=>tab(b.dataset.ccTab);
  $('ci-filter').oninput=drawInv;
  $('show-cc').addEventListener('click',()=>tab('projects'));
  window.AppCcTabs={select:tab};
})();
