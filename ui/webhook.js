'use strict';
(() => {
  const $=id=>document.getElementById(id);
  const invoke=(c,a={})=>AppIPC.invoke(c,a);
  const EV={quiet:'Quiet',error:'Error',finished:'Finished',waiting:'Waiting',limit:'Plan limit'};
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
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  async function check(){
    let s;try{s=await invoke('codex_hook_status');}catch(e){$('codexcfg-state').textContent=String(e);return;}
    $('codexcfg-snippet').value=s.snippet;
    $('codexcfg-state').textContent=s.last_at?`Last signal: ${new Date(s.last_at*1000).toLocaleString()}. The notify line works.`:'No signal received yet. After adding the line, restart Codex and finish one turn; then press "Check for a signal".';
  }
  $('codexcfg-check').onclick=check;
  $('codexcfg-copy').onclick=()=>{const t=$('codexcfg-snippet');t.focus();t.select();try{document.execCommand('copy');$('codexcfg-state').textContent='Copied. Paste it into ~/.codex/config.toml.';}catch{$('codexcfg-state').textContent='Select the text and copy it.';}};
  $('show-settings').addEventListener('click',check);
})();
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  function show(s){
    $('desk-on').checked=s.enabled;$('desk-on').disabled=!s.available&&!s.enabled;$('desk-test').disabled=!s.available;
    $('desk-state').textContent=!s.available?'notify-send was not found on this computer (it comes with libnotify).':(s.last||(s.enabled?'On.':'Off.'));
  }
  async function run(a){try{show(await invoke('desktop_notify',a));}catch(e){$('desk-state').textContent=String(e);}}
  $('desk-on').onchange=()=>run({enabled:$('desk-on').checked});
  $('desk-test').onclick=()=>run({test:true});
  $('show-settings').addEventListener('click',()=>run({}));
})();
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  const fmtAge=s=>s<90?'just now':s<5400?`${Math.round(s/60)} min ago`:`${Math.round(s/3600)} h ago`;
  function show(s){
    $('limits-snippet').value=s.snippet;$('limits-th').value=String(s.threshold||0);
    const l=s.limits;
    $('limits-state').textContent=l?`Last data ${fmtAge(s.age_s)}: ${[l.five&&`5-hour ${Math.round(l.five.pct)}%`,l.seven&&`7-day ${Math.round(l.seven.pct)}%`].filter(Boolean).join(', ')}. ${s.threshold?`Alert at ${s.threshold}%.`:'Alert is off.'}`:'No data yet. After adding the line, send one message in a Claude Code session, then press "Check for data".';
  }
  async function run(a){try{show(await invoke('claude_limits_status',a));}catch(e){$('limits-state').textContent=String(e);}}
  $('limits-check').onclick=()=>run({});
  $('limits-th').onchange=()=>run({threshold:Number($('limits-th').value)});
  $('limits-copy').onclick=()=>{const t=$('limits-snippet');t.focus();t.select();try{document.execCommand('copy');$('limits-state').textContent='Copied. Add it to your status line script.';}catch{$('limits-state').textContent='Select the text and copy it.';}};
  $('show-settings').addEventListener('click',()=>run({}));
})();

(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  async function check(){
    $('mail-state').textContent='';
    try{const s=await invoke('mailbox_status',{name:$('mail-name').value.trim()});$('mail-send').value=s.send_snippet;$('mail-recv').value=s.receive_snippet;$('mail-state').textContent=`${s.count} recent message${s.count===1?'':'s'} in the mailbox.`;}
    catch(e){$('mail-state').textContent=window.friendly('Could not show the mailbox snippets',e);return;}
    let list=[];try{list=await invoke('mailbox_list');}catch{}
    const box=$('mail-recent');box.replaceChildren();
    for(const m of list.slice().reverse().slice(0,10)){const p=document.createElement('p');p.textContent=`${new Date(m.t*1000).toLocaleString()} · ${m.from} → ${m.to}: ${m.text}`;box.append(p);}
  }
  $('mail-check').onclick=check;$('show-settings').addEventListener('click',check);
})();
