'use strict';
(() => {
  const $=id=>document.getElementById(id);
  const invoke=(c,a={})=>AppIPC.invoke(c,a);
  const EV={quiet:'Quiet',error:'Error',finished:'Finished',waiting:'Waiting'};
  let editing=null,removing=null,lastText='';
  function button(text,fn){const b=document.createElement('button');b.type='button';b.textContent=text;b.onclick=fn;return b;}
  function form(){
    $('hook-save').textContent=editing?'Save changes':'Add destination';$('hook-cancel').hidden=!editing;
    $('hook-url').placeholder=editing?'Leave empty to keep the saved address':'Paste an https:// address';
  }
  function resetForm(){editing=null;$('hook-name').value='';$('hook-url').value='';for(const k of Object.keys(EV))$('hook-ev-'+k).checked=k!=='finished';$('hook-enabled').checked=true;form();}
  function show(s){
    lastText=s.last||'';const body=$('hook-rows');body.replaceChildren();
    for(const h of s.hooks){
      const tr=document.createElement('tr');
      for(const t of [h.name,h.host||'unreadable',h.events.map(e=>EV[e]||e).join(', '),h.enabled?'On':'Off']){const td=document.createElement('td');td.textContent=t;tr.append(td);}
      const act=document.createElement('td');
      act.append(button('Test',()=>test(h.id)),button('Edit',()=>{editing=h.id;$('hook-name').value=h.name;$('hook-url').value='';for(const k of Object.keys(EV))$('hook-ev-'+k).checked=h.events.includes(k);$('hook-enabled').checked=h.enabled;form();}));
      if(removing===h.id)act.append(button('Confirm remove',()=>run('webhook_clear',{id:h.id}).then(()=>{removing=null;})));
      else act.append(button('Remove',()=>{removing=h.id;load();}));
      tr.append(act);body.append(tr);
    }
    $('hook-table').hidden=!s.hooks.length;
    $('hook-state').textContent=(s.hooks.length?`${s.hooks.length} destination(s).`:'No destinations yet.')+(lastText?` ${lastText}.`:'');
  }
  async function run(cmd,args){try{show(await invoke(cmd,args));return true;}catch(e){$('hook-state').textContent=String(e);return false;}}
  const load=()=>run('webhook_status');
  async function test(id){$('hook-state').textContent='Sending…';try{$('hook-state').textContent=await invoke('webhook_test',{id});}catch(e){$('hook-state').textContent=String(e);}}
  $('hook-save').onclick=async()=>{
    const events=Object.keys(EV).filter(k=>$('hook-ev-'+k).checked);
    if(await run('webhook_save',{id:editing,name:$('hook-name').value,url:$('hook-url').value,enabled:$('hook-enabled').checked,events}))resetForm();
  };
  $('hook-cancel').onclick=resetForm;
  $('show-settings').addEventListener('click',()=>{resetForm();removing=null;load();});
})();
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  async function check(){
    let s;try{s=await invoke('claude_hook_status');}catch(e){$('hookcfg-state').textContent=String(e);return;}
    $('hookcfg-snippet').value=s.snippet;
    $('hookcfg-state').textContent=s.last_at?`Last signal: ${new Date(s.last_at*1000).toLocaleString()} (${s.last_kind||'unknown'}). The hook works.`:'No signal received yet. After adding the hook, the next time Claude asks for permission (or waits) a line appears here.';
  }
  $('hookcfg-check').onclick=check;
  $('hookcfg-copy').onclick=()=>{const t=$('hookcfg-snippet');t.focus();t.select();try{document.execCommand('copy');$('hookcfg-state').textContent='Copied. Paste it into your Claude settings.';}catch{$('hookcfg-state').textContent='Select the text and copy it.';}};
  $('show-settings').addEventListener('click',check);
})();
