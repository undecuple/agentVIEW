// Files accessed: reads the app's private log that the user's own Claude Code PostToolUse hook appends to. The app never opens the files named in it.
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  let timer=null;
  async function status(){
    let s;try{s=await invoke('files_access_status');}catch(e){$('filescfg-state').textContent=String(e);return;}
    $('filescfg-snippet').value=s.snippet;
    $('filescfg-state').textContent=s.last_at?`Last file access logged: ${new Date(s.last_at*1000).toLocaleString()}. The hook works.`:'Nothing logged yet. After adding the hook, the next time Claude reads or edits a file it appears here and on the Flow page.';
  }
  async function load(){
    const kind=$('files-kind').value;let rows;
    try{rows=await invoke('files_access_report',{limit:150});}catch(e){$('files-empty').textContent=String(e);$('files-empty').hidden=false;$('files-table').hidden=true;return;}
    if(kind!=='all')rows=rows.filter(r=>r.kind===kind);
    const body=$('files-body');body.replaceChildren();
    $('files-empty').textContent='Nothing logged yet. Add the hook in Settings (Claude Code hook: files accessed).';
    $('files-empty').hidden=rows.length>0;$('files-table').hidden=!rows.length;
    for(const r of rows){const tr=document.createElement('tr');
      for(const v of [new Date(r.t*1000).toLocaleTimeString(),r.session.slice(0,8),r.project||'',r.kind==='read'?'Read':'Changed ('+r.tool+')',r.path]){const td=document.createElement('td');td.textContent=v;tr.append(td);}
      tr.lastChild.className='mono';if(r.hidden)tr.lastChild.className+=' muted';body.append(tr);}
  }
  const start=()=>{stop();load();timer=setInterval(()=>{if(!document.hidden)load();},5000);};
  const stop=()=>{if(timer){clearInterval(timer);timer=null;}};
  $('files-kind').onchange=load;$('files-reload').onclick=load;
  $('filescfg-check').onclick=status;
  $('filescfg-copy').onclick=()=>{const t=$('filescfg-snippet');t.focus();t.select();try{document.execCommand('copy');$('filescfg-state').textContent='Copied. Paste it into your Claude settings.';}catch{$('filescfg-state').textContent='Select the text and copy it.';}};
  AppNav.onShow('settings',status);
  window.addEventListener('agentview:navigate',e=>{if(e.detail==='flow')start();else stop();});
})();
