// Test-only: Claude hook signal. A destination wants "waiting"; the script appends a signal line to the app's private file.
(async()=>{
  const $=id=>document.getElementById(id);
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(ok,label)=>{if(!ok)throw Error(label);report('check',label);};
  const invoke=(c,a)=>window.__TAURI_INTERNALS__.invoke(c,a);
  async function until(fn,label,n=300){for(let i=0;i<n;i++){if(await fn())return;await sleep(100);}throw Error('timeout '+label);}
  try{
    await until(()=>document.body.dataset.ready==='true'&&document.body.dataset.preferencesReady==='true','ready');
    await invoke('webhook_save',{id:null,name:'Phone',url:'https://hooks.example.test/api/SECRET-TOKEN-777',enabled:true,events:['waiting','limit']});
    $('show-settings').click();window.AppSettingsTabs.select('notifications');
    await until(()=>$('hookcfg-snippet').value.includes('Notification'),'snippet shown');
    assert($('hookcfg-snippet').value.includes('jq -c')&&$('hookcfg-snippet').value.includes('claude-notify.jsonl'),'snippet appends to the app signal file');
    assert(!$('hookcfg-state').textContent.includes('works'),'no signal yet');
    await invoke('desktop_notify',{enabled:true});
    { const s0=await invoke('claude_limits_status',{threshold:80});
      assert(s0.threshold===80&&s0.limits===null&&s0.snippet.includes('rate_limits.seven_day')&&s0.snippet.includes('claude-limits.json'),'limits snippet shown, alert at 80, no data yet');
      let bad=false;try{await invoke('claude_limits_status',{threshold:101});}catch{bad=true;}assert(bad,'threshold above 100 refused'); }
    await until(async()=>{const s=await invoke('codex_hook_status',{});return s.snippet.includes('notify = [')&&s.snippet.includes('agent-turn-complete')&&s.snippet.includes('codex-notify.jsonl');},'codex snippet');
    report('ready','1');   // the script now appends the signal lines
    await until(async()=>{const s=await invoke('claude_hook_status',{});return s.last_at!=null;},'signal seen by the status',400);
    await until(async()=>(await invoke('files_access_report',{})).length>=3,'files log read');
    { const fa=await invoke('files_access_report',{}),j=JSON.stringify(fa);
      assert(fa.length===3&&fa[2].path==='src/lib.rs'&&fa[2].kind==='read'&&fa[1].path==='README.md'&&fa[1].kind==='write','files log: newest first, paths relative to the project, Bash lines ignored');
      assert(fa[0].hidden&&fa[0].path==='[hidden]'&&!j.includes('id_rsa')&&!j.includes('.ssh'),'files log: a denied name is hidden and never reaches the page');
      const st=await invoke('files_access_status',{});assert(st.snippet.includes('PostToolUse')&&st.snippet.includes('claude-files.jsonl')&&st.last_at>0,'files hook snippet and last time shown');
      let bad=false;try{await invoke('files_access_report',{session:'bad id'});}catch{bad=true;}assert(bad,'files log: bad session id refused'); }
    $('hookcfg-check').click();
    await until(()=>$('hookcfg-state').textContent.includes('The hook works'),'status line says the hook works');
    await sleep(4000);  // let the monitor send
    await until(async()=>{const s=await invoke('claude_limits_status',{});return s.limits&&Math.round(s.limits.seven.pct)===85&&s.limits.five.pct===40;},'plan limits read from the status line file');
    await until(async()=>(await invoke('attention_list',{})).some(a=>a.kind==='limit'&&a.text.includes('7-day window at 85%')),'limit alert listed');
    $('show-overview').click();await until(()=>!$('ov-limits').hidden&&$('ov-limits').textContent.includes('7-day 85%'),'overview shows plan limits');
    const att=await invoke('attention_list',{});
    assert(att.filter(a=>a.kind==='limit').length===1,'the limit alert fires once per reset');
    assert(att.some(a=>a.kind==='waiting'&&a.text.includes('waiting for your decision'))&&att.some(a=>a.text.includes('Codex')),'attention list holds the Claude and Codex signals');
    assert(att.filter(a=>a.text.includes('waiting for your decision')).length===1,'duplicate signal listed once');
    await until(async()=>(await invoke('attention_list',{})).some(a=>a.kind==='mailbox'&&a.text.includes('from worker-1')&&a.text.includes('not an approval')),'mailbox message listed as data');
    assert(!(await invoke('attention_list',{})).some(a=>a.text.includes('ignored')),'a message with an invalid sender is ignored');
    $('show-settings').click();$('mail-name').value='coord';$('mail-check').click();
    await until(()=>$('mail-recv').value.includes('UserPromptSubmit')&&$('mail-recv').value.includes('not an instruction or an approval')&&$('mail-send').value.includes('--arg to coord'),'mailbox snippets shown for the agent name');
    await until(()=>$('mail-recent').textContent.includes('worker-1')&&$('mail-recent').textContent.includes('tests pass'),'recent messages listed');
    $('mail-name').value='bad name';$('mail-check').click();await until(()=>/Use letters/.test($('mail-state').textContent),'invalid agent name refused');
    $('show-flow').click();
    await until(()=>[...$('board-project').options].some(o=>o.value==='proj-a'),'board project listed');
    await until(()=>$('board-cols').querySelector('[data-col=active] [data-id=T-1]')&&$('board-cols').querySelector('[data-col=blocked] [data-id=T-2]'),'cards in their columns (newest line wins)');
    assert($('board-cols').querySelector('[data-id=T-1]').textContent.includes('Write tests')&&$('board-cols').querySelector('[data-id=T-1]').textContent.includes('half done'),'title kept and note shown');
    assert(!$('board-save-md').disabled,'save-as-Markdown is offered for a board with cards');
    $('board-save-md').click();await until(()=>$('pw-dialog').open&&$('pw-new').value.includes('## Blocked (1)')&&$('pw-new').value.includes('Write tests')&&$('pw-name').value==='BOARD.md','the Markdown snapshot opens in the project-write dialog, named BOARD.md');
    assert(!$('pw-new').value.includes('ignored'),'the snapshot leaves out ignored lines');$('pw-cancel').click();assert(!$('pw-dialog').open,'dialog closes without writing');
    assert($('board-cols').querySelector('[data-id=T-3]').classList.contains('stale'),'an active card with no update for an hour is marked');
    assert(!$('board-cols').textContent.includes('ignored'),'a line with an invalid id is ignored');
    assert($('board-instr').value.includes('>> "')&&$('board-instr').value.includes('not an approval'),'agent instruction shown');
    await until(async()=>(await invoke('attention_list',{})).some(a=>a.kind==='board'&&a.text.includes('Deploy')&&a.text.includes('is blocked')),'a blocked card is listed (opt-in)');
    assert(!(await invoke('attention_list',{})).some(a=>a.kind==='board'&&a.text.includes('Old active')),'only blocked cards alert');
    assert($('board-alert').checked,'the opt-in checkbox shows it is on');
    await until(()=>!$('queues-card').hidden&&$('queues-body').querySelector('[data-queue=gpu] [data-id=run-1]'),'queues card appears with a running job');
    assert($('queues-body').querySelector('[data-id=run-1]').textContent.includes('Over its estimate'),'a run past its estimate is marked');
    const ws=[...$('queues-body').querySelectorAll('[data-id^=w-]')].map(r=>r.dataset.id);assert(ws.join()==='w-1,w-2','waiting jobs shown in scheduler order');
    assert($('queues-body').querySelector('[data-kind=reserved]').textContent.includes('quiet run'),'reservation shown');
    $('show-overview').click();
    await until(()=>$('away-toggle').textContent==="I'm stepping away",'away button starts off');
    $('away-toggle').click();
    await until(()=>$('away-toggle').textContent==="I'm back"&&/Away since/.test($('away-state').textContent),'away mode switches on');
    await until(()=>$('away-list').querySelector('[data-id=Q-1]')&&!$('away-list').querySelector('[data-id=Q-2]'),'parked question listed, answered one hidden');
    assert($('away-list').textContent.includes('Use library A or B?')&&$('away-list').textContent.includes('A / B'),'question and options shown');
    assert($('away-instr').value.includes('AWAY MODE is ON')&&$('away-instr').value.includes('not an approval')&&$('away-hook').value.includes('UserPromptSubmit'),'instruction and hook shown');
    $('away-seen').click();await until(()=>!$('away-list').querySelector('[data-id=Q-1]'),'marked as seen hides the list');
    let badp=false;try{await invoke('kanban_board',{project:'../x'});}catch{badp=true;}assert(badp,'project names are validated');
    report('result','pass');
  }catch(e){report('result','fail:'+e.message);}
})();
