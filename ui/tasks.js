// Tasks timeline and Claude Code workflow runs: reads the app's private log that the user's own UserPromptSubmit and Stop hooks append to.
// Workflow runs come from Claude Code's own run records; only state, counts and times are shown, never prompts or outputs.
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  let timer=null;
  const dur=s=>{if(s==null)return '';if(s<60)return s+' s';const m=Math.floor(s/60);return m<60?m+' min '+(s%60)+' s':Math.floor(m/60)+' h '+(m%60)+' min';};
  const day=t=>{const d=new Date(t*1000);return d.getFullYear()+'-'+String(d.getMonth()+1).padStart(2,'0')+'-'+String(d.getDate()).padStart(2,'0');};
  const num=n=>Number(n).toLocaleString();
  const size=n=>n<1024?n+' bytes':n<1048576?(n/1024).toFixed(1)+' KB':(n/1048576).toFixed(1)+' MB';
  const cells=(tr,vals)=>{for(const v of vals){const td=document.createElement('td');td.textContent=v;tr.append(td);}return tr;};
  async function status(){
    let s;try{s=await invoke('tasks_status');}catch(e){$('taskscfg-state').textContent=String(e);return;}
    $('taskscfg-snippet').value=s.snippet;
    const sz=s.exists?` Log file: ${size(s.size)}; it only grows, delete it to clear the history.`:'';
    $('taskscfg-state').textContent=(s.last_at?`Last task logged: ${new Date(s.last_at*1000).toLocaleString()}. The hooks work.`:'Nothing logged yet. After adding the hooks, the next prompt you send appears here and on the Flow page.')+sz;
  }
  function totals(rows){
    const by=new Map();
    for(const r of rows){const d=day(r.start),e=by.get(d)||{n:0,s:0};e.n++;if(r.duration!=null)e.s+=r.duration;by.set(d,e);}
    const today=day(Date.now()/1000),parts=[];
    for(const [d,e] of [...by].slice(0,4))parts.push((d===today?'Today':d)+': '+e.n+(e.n===1?' task, ':' tasks, ')+(dur(e.s)||'0 s')+' finished');
    $('tasks-totals').textContent=parts.join('   ·   ');
  }
  async function loadTasks(){
    let rep;
    try{rep=await invoke('tasks_report',{limit:150});}catch(e){$('tasks-empty').textContent=String(e);$('tasks-empty').hidden=false;$('tasks-table').hidden=true;return;}
    const rows=rep.rows,body=$('tasks-body');body.replaceChildren();
    $('tasks-empty').textContent='Nothing logged yet. Add the hooks in Settings (Claude Code hooks: tasks).';
    $('tasks-empty').hidden=rows.length>0;$('tasks-table').hidden=!rows.length;totals(rows);
    for(const r of rows){const k=r.tokens,fresh=k?k.input+k.output+k.cache_creation:0;
      const tr=cells(document.createElement('tr'),[new Date(r.start*1000).toLocaleString(),r.project||'',r.label,r.duration==null?'':dur(r.duration),k?num(fresh)+(k.cache_read?' + '+num(k.cache_read)+' cached':''):(r.end==null?'':'n/a'),r.state]);
      tr.children[2].className='mono';
      tr.children[4].title=k?`Input ${num(k.input)}, output ${num(k.output)}, cache written ${num(k.cache_creation)}; cache read ${num(k.cache_read)} (shown apart). ${rep.tokens_note}`:(r.tokens_why||rep.tokens_note);
      body.append(tr);}
  }
  async function loadRuns(){
    let runs,rep;
    try{rep=await invoke('workflows_report',{limit:30});runs=rep.runs;}catch(e){$('wfruns-empty').textContent=String(e);$('wfruns-empty').hidden=false;$('wfruns-table').hidden=true;return;}
    const body=$('wfruns-body');body.replaceChildren();
    $('wfruns-empty').textContent='No workflow runs found.';
    $('wfruns-empty').hidden=runs.length>0;$('wfruns-table').hidden=!runs.length;
    $('wfruns-note').textContent=rep.note+(rep.skipped?` ${rep.skipped} record${rep.skipped===1?'':'s'} could not be read and ${rep.skipped===1?'was':'were'} skipped.`:'')+(rep.truncated?' Folder limits were reached, so older runs are not listed.':'');
    for(const r of runs){const tr=cells(document.createElement('tr'),[r.start?new Date(r.start*1000).toLocaleString():'',r.id,r.name,r.state,r.agents==null?'':String(r.agents),r.duration_ms==null?'':dur(Math.round(r.duration_ms/1000)),r.tokens==null?'':r.tokens.toLocaleString()]);
      tr.children[1].className='mono';body.append(tr);}
  }
  const load=()=>{loadTasks();loadRuns();};
  const start=()=>{stop();load();timer=setInterval(()=>{if(!document.hidden)loadTasks();},5000);};
  const stop=()=>{if(timer){clearInterval(timer);timer=null;}};
  $('tasks-reload').onclick=load;
  $('taskscfg-check').onclick=status;
  $('taskscfg-copy').onclick=()=>{const t=$('taskscfg-snippet');t.focus();t.select();try{document.execCommand('copy');$('taskscfg-state').textContent='Copied. Paste it into your Claude settings.';}catch{$('taskscfg-state').textContent='Select the text and copy it.';}};
  AppNav.onShow('settings',status);
  window.addEventListener('agentview:navigate',e=>{if(e.detail==='flow')start();else stop();});
})();
