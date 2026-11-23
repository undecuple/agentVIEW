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
