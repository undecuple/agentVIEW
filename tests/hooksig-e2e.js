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
    await invoke('webhook_save',{id:null,name:'Phone',url:'https://hooks.example.test/api/SECRET-TOKEN-777',enabled:true,events:['waiting']});
    $('show-settings').click();window.AppSettingsTabs.select('notifications');
    await until(()=>$('hookcfg-snippet').value.includes('Notification'),'snippet shown');
    assert($('hookcfg-snippet').value.includes('jq -c')&&$('hookcfg-snippet').value.includes('claude-notify.jsonl'),'snippet appends to the app signal file');
    assert(!$('hookcfg-state').textContent.includes('works'),'no signal yet');
    report('ready','1');   // the script now appends the signal lines
    await until(async()=>{const s=await invoke('claude_hook_status',{});return s.last_at!=null;},'signal seen by the status',400);
    $('hookcfg-check').click();
    await until(()=>$('hookcfg-state').textContent.includes('The hook works'),'status line says the hook works');
    await sleep(4000);  // let the monitor send
    report('result','pass');
  }catch(e){report('result','fail:'+e.message);}
})();
