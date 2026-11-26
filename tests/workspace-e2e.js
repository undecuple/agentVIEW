// Appended only to the workspace-e2e debug feature. Exercises the real embedded UI.
(async () => {
  const $=id=>document.getElementById(id), invoke=(c,a={})=>window.__TAURI_INTERNALS__.invoke(c,a);
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const assert=(condition,label)=>{if(!condition)throw Error(label);report('check',label);};
  async function until(fn,label) {for(let i=0;i<200;i++){if(await fn())return;await new Promise(r=>setTimeout(r,100));}throw Error(`timeout ${label}; notice=${$('notice')?.textContent}`);}
  const snap=()=>invoke('workspace_snapshot',{agentId:'codex-app-coord'});
  try {
    await until(()=>document.body.dataset.ready==='true'&&document.body.dataset.preferencesReady==='true','workspace ready');
    await AppPrefs.patch({demo_mode:true});await until(()=>document.body.dataset.demo==='true','demo workspace enabled for this test');AppNav.navigate('agents');
    if(TEST_NARROW)assert(window.innerWidth<=800,'narrow viewport exercised');
    assert($('show-docs').getClientRects().length>0&&$('show-usage').getClientRects().length>0,'documents and usage navigation remain visible');
    assert(document.querySelectorAll('.agent-row').length===8,'both intake trees and selected project descendants rendered');
    $('project-filter').value='';$('project-filter').dispatchEvent(new Event('input'));
    assert(document.querySelectorAll('.agent-row').length===14,'all projects available');
    report('capture','hierarchy');await new Promise(r=>setTimeout(r,1400));
    $('project-filter').value='Agent App';$('project-filter').dispatchEvent(new Event('input'));
    const s=await snap();assert(s.agents.filter(a=>a.role==='intake').length===2,'separate intake roots');
    $('project-view').click();assert($('roster').textContent.includes('AGENT APP'),'project groups rendered');
    assert($('roster').textContent.includes('Claude /')&&$('roster').textContent.includes('Codex /'),'both provider coordinators grouped');
    $('tree-view').click();
    $('search').value='app-worker';$('search').dispatchEvent(new Event('input'));
    assert(document.querySelectorAll('.agent-row').length===6,'search retains ancestor context');
    $('search').value='';$('search').dispatchEvent(new Event('input'));
    $('show-usage').click();assert(!$('usage').hidden&&$('deck').hidden,'usage view opens');
    await until(()=>$('usage-notice').textContent.includes('Showing saved'),'stored usage loaded');
    assert($('usage-agents').textContent.includes('No registered live sessions')&&$('usage').textContent.includes('Unavailable'),'simulated agents do not receive token measurements');
    report('capture','usage');await new Promise(r=>setTimeout(r,1400));
    $('close-usage').click();
    assert($('agent-name').textContent==='app-coordinator','default selected coordinator');
    if(TEST_PHASE==='first') {
      assert(s.messages.length===0,'isolated new conversation');
      document.querySelector('[data-state=busy]').click();
      await until(async()=>(await snap()).agents.find(a=>a.id==='codex-app-coord').availability==='busy','busy saved');
      $('message').value='Persisted GUI <script>window.pwned=1</script>';
      $('composer').dispatchEvent(new Event('submit',{cancelable:true}));
      await until(async()=>(await snap()).messages.length===1,'composer saved');
      await until(()=>$('messages').textContent.includes('Persisted GUI'),'message rendered');
      assert(!window.pwned&&!$('messages').querySelector('script'),'HTML rendered as text');
      assert((await snap()).messages[0].state==='queued','busy recipient remains queued');
      report('phase','first-restart-ready');
    } else {
      assert(s.messages.length===1&&s.messages[0].state==='queued','queued message survives process restart');
      assert(s.agents.find(a=>a.id==='codex-app-coord').availability==='busy','availability survives process restart');
      document.querySelector('[data-state=idle]').click();
      await until(async()=>(await snap()).messages.length===2,'resumed queued reply');
      const delivered=await snap();assert(delivered.messages[0].state==='delivered'&&delivered.messages[1].reply_to===delivered.messages[0].id,'exactly one reply to persisted message');
      $('message').value='Continue after restart';$('composer').dispatchEvent(new Event('submit',{cancelable:true}));
      await until(async()=>(await snap()).messages.length===4,'continued conversation');
      await until(()=>$('messages').querySelectorAll('.message').length===4,'continued conversation rendered');
      await invoke('workspace_tick');assert((await snap()).messages.length===4,'extra tick produces no duplicate');
      report('phase','restart-preview'); await new Promise(r=>setTimeout(r,2500));
      $('open-docs').click();
      await until(()=>new Promise(resolve=>{const script=document.createElement('script');script.src='/__workspace-docs-status.js';script.onload=()=>{script.remove();resolve(window.__docsDenied);};script.onerror=()=>{script.remove();resolve(false);};document.head.append(script);}),'docs rejects all workspace and session commands');
      await invoke('workspace_control',{agentId:'codex-app-coord',availability:'busy'});
      let firstQueued;
      for(let i=0;i<105;i++) {
        const id=await invoke('workspace_send',{agentId:'codex-app-coord',requestId:`pagination-${i}`,body:`History regression ${i}`});
        if(i===0)firstQueued=id;
      }
      await until(()=>$('messages').querySelectorAll('.message').length===100&&!$('load-older').hidden,'bounded latest page');
      $('load-older').click();await until(()=>$('messages').querySelectorAll('.message').length===109,'older page loaded');
      await invoke('workspace_control',{agentId:'codex-app-coord',availability:'idle'});await invoke('workspace_tick');
      await until(()=>$('messages').querySelector(`[data-message-id="${firstQueued}"] .message-state`)?.textContent.includes('Delivered'),'older loaded message updates after delivery');
      assert(true,'older loaded queue state refreshed');
      report('phase','restart-complete');
    }
    // Allow the runner to capture the actual workspace before ending this process.
    await new Promise(r=>setTimeout(r,2500));report('result','pass');
  } catch(e) {report('error',String(e));report('result','fail');}
})();
