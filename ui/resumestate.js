'use strict';
// Resume state and handoff (Overview). Read-only: file dates from the Claude memory listing and the activity data the Overview already has.
// The handoff note is built here from facts the app can see (folder, state, git changes, notes file names); it contains no conversation text and no terminal output.
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  let timer=null,acts=[],mem=[],ticks=0,open=null;
  const slug=p=>(p||'').replace(/[^A-Za-z0-9]/g,'-');
  const base=p=>(p||'').split('/').filter(Boolean).pop()||'';
  const dur=ms=>{const m=Math.round(ms/60000);return m<1?'under a minute':m<90?`${m} min`:m<2880?`${Math.round(m/60)} h`:`${Math.round(m/1440)} days`;};
  const providerName=p=>p==='codex'?'Codex':'Claude Code';
  function notesFor(a){
    const sl=slug(a.project),b=base(a.project);
    return mem.filter(e=>e.key.startsWith('m|'+sl+'|')||(e.key.startsWith('a|')&&e.group.split(/[\s/]/).pop()===b));
  }
  const lastActivity=a=>a.running?Date.now()-(a.idle_ms??0):(a.exit_at&&!Number.isNaN(Date.parse(a.exit_at))?Date.parse(a.exit_at):null);
  function status(a,files){
    if(!files.length)return a.provider==='codex'?'No notes file visible (Codex keeps none the app can see unless the folder is allow-listed)':'No notes found';
    const newest=Math.max(...files.map(f=>f.modified))*1000,la=lastActivity(a),age=dur(Date.now()-newest);
    if(la==null||newest>=la-5*60000)return `Looks current (newest notes file changed ${age} ago)`;
    return `Older than the last activity by ${dur(la-newest)} (newest notes file changed ${age} ago)`;
  }
  function render(){
    const body=$('rs-body');body.replaceChildren();$('rs-empty').hidden=acts.length>0;$('rs-table').hidden=!acts.length;
    for(const a of acts){
      const files=notesFor(a),tr=document.createElement('tr');
      const td=t=>{const c=document.createElement('td');c.textContent=t;tr.append(c);return c;};
      td(`${a.display_name}`);td(files.length?files.map(f=>f.name).slice(0,6).join(', ')+(files.length>6?` +${files.length-6}`:''):'—');
      const st=td(status(a,files));st.dataset.stale=String(/^Older/.test(st.textContent));
      const act=document.createElement('td'),b=document.createElement('button');b.type='button';b.textContent='Handoff note';b.onclick=()=>openHandoff(a,files);act.append(b);tr.append(act);body.append(tr);
    }
  }
  async function openHandoff(a,files){
    open=a.id;$('rs-handoff').hidden=false;$('rs-note').textContent='Building…';
    $('rs-target').value=a.provider==='codex'?'Claude Code':'Codex';
    let ch=null,chErr='';try{ch=await invoke('session_changes',{id:a.id});}catch(e){chErr=String(e);}
    const L=[];const now=new Date();
    L.push(`# Handoff: ${a.display_name}`,'',`Prepared ${now.toISOString().slice(0,16).replace('T',' ')} UTC by agentVIEW from facts it can observe. The agent's own notes files below are the authority; this page is not a record of the conversation.`,'');
    L.push('## Where things stand',`- Provider that was working: ${providerName(a.provider)}`,`- Folder: ${a.project||'unknown'}`,`- Process: ${a.running?'running':`not running${a.exit_info?` (${a.exit_info})`:''}`}`);
    if(a.action)L.push(`- Last tool use seen: ${a.action}`);
    L.push('','## Git');
    if(ch){L.push(`- Repository: ${ch.repo}, branch ${ch.branch||'unknown'}`,`- Changed files now: ${ch.total}${ch.hidden_by_policy?` (+${ch.hidden_by_policy} hidden by deny rules)`:''}`);
      for(const f of ch.files.slice(0,30))L.push(`  - ${f.state} ${f.path}`);
      if(ch.files.length>30)L.push(`  - … and ${ch.total-30} more`);
      L.push(ch.since_s!=null?`- Commits since this session was started in this app run: ${ch.commits.length}`:`- Latest commits (start time unknown after a restart):`);
      for(const k of ch.commits.slice(0,15))L.push(`  - ${k.id} ${k.subject}`);}
    else L.push(`- Not available: ${chErr||'unknown'}`);
    L.push('','## Read these first');
    if(files.length)for(const f of files)L.push(`- ${f.group}: ${f.name} (changed ${dur(Date.now()-f.modified*1000)} ago)`);
    else L.push('- No notes files were visible to the app. Ask the previous agent where its notes are.');
    L.push(`- The project rules file: ${$('rs-target').value==='Codex'?'AGENTS.md':'CLAUDE.md'} in the folder above, if it exists`);
    L.push('','## First steps for the next agent','- Read the files above, then say in five lines what you understand the state to be and what you will do first.','- Do not change anything until the person confirms.','','## Not included','Conversation text, terminal output, credentials and environment values are never part of this note.');
    $('rs-text').value=L.join('\n');$('rs-note').textContent='Review it before you use it.';
  }
  function retarget(){const t=$('rs-text').value;if(!t)return;const to=$('rs-target').value==='Codex'?'AGENTS.md':'CLAUDE.md',other=to==='AGENTS.md'?'CLAUDE.md':'AGENTS.md';$('rs-text').value=t.replace(`- The project rules file: ${other}`,`- The project rules file: ${to}`);}
  async function load(){
    if(document.hidden||$('overview').hidden)return;
    try{acts=await invoke('live_activity');}catch{return;}
    if(ticks++%15===0)try{mem=await invoke('claude_memory_list');}catch{mem=[];}
    render();
  }
  $('rs-target').onchange=retarget;
  $('rs-close').onclick=()=>{$('rs-handoff').hidden=true;$('rs-text').value='';open=null;};
  $('rs-copy').onclick=()=>{const t=$('rs-text');t.focus();t.select();try{document.execCommand('copy');$('rs-note').textContent='Copied.';}catch{$('rs-note').textContent='Select the text and copy it.';}};
  $('rs-project').onclick=()=>{if(!open||!$('rs-text').value)return;window.ProjWrite.open({content:$('rs-text').value,name:'HANDOFF.md',names:['HANDOFF.md']});};
  $('rs-save').onclick=async()=>{
    const a=acts.find(x=>x.id===open);if(!a||!$('rs-text').value)return;
    const title=`Handoff ${a.display_name} ${new Date().toISOString().slice(0,10)}`.slice(0,80);
    try{await invoke('notes_save',{id:null,shelf:'Handoffs',title,body:$('rs-text').value,baseUpdated:null});$('rs-note').textContent='Saved to Notes (shelf "Handoffs").';}catch(e){$('rs-note').textContent=String(e);}
  };
  window.addEventListener('agentview:navigate',e=>{clearInterval(timer);timer=null;if(e.detail==='overview'){ticks=0;load();timer=setInterval(load,4000);}});
})();
