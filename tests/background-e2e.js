// Only served in workspace-e2e (AGENT_APP_TEST_BG). Close-window behaviour with a running synthetic session.
// BG_MODE 'default': the preference is left alone (stop); the window close must stop the session and quit.
// BG_MODE 'background': the preference is saved, the window close must hide; the shell script then runs the app a second time
// (show) and later with --quit. This page keeps running while hidden and checks what the user sees after "show".
(async()=>{
  const $=id=>document.getElementById(id),invoke=(c,a={})=>window.__TAURI_INTERNALS__.invoke(c,a);
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const assert=(ok,label)=>{if(!ok)throw Error(label);report('check',label);};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  async function until(fn,label,tries=300){for(let i=0;i<tries;i++){if(await fn())return;await sleep(100);}throw Error(`timeout ${label}`);}
  async function output(s,offset=0){const r=await invoke('live_read',{id:s.id,generation:s.generation,offset});return {r,text:new TextDecoder().decode(Uint8Array.from(atob(r.data),c=>c.charCodeAt(0)))};}
  const shown=()=>new Promise(resolve=>{const s=document.createElement('script');s.src='/__workspace-bg-status.js?'+Math.random();s.onload=()=>{s.remove();resolve(window.__bgShown);};s.onerror=()=>{s.remove();resolve(0);};document.head.append(s);});
  try{
    await until(()=>typeof AppNav!=='undefined'&&typeof AppPrefs!=='undefined'&&document.readyState==='complete','page scripts');
    await AppPrefs.ready;AppNav.navigate('live');
    await invoke('live_probe',{provider:'claude'});
    const created=await invoke('live_create',{provider:'claude',label:'background Claude',projectDir:`${PROJECT_ROOT}/alpha`});
    const s=await invoke('live_start',{id:created.id});
    await until(async()=>(await output(s)).text.includes('FIXTURE READY'),'fixture output');
    if(BG_MODE==='background'){
      const snap=await invoke('app_settings_snapshot');
      assert(snap.settings.close_behavior==='stop','default close_behavior is stop');
      await invoke('app_settings_save',{expectedRevision:snap.revision,settings:{...snap.settings,close_behavior:'background'}});
      assert((await invoke('app_settings_snapshot')).settings.close_behavior==='background','background preference saved');
    }
    await until(()=>document.querySelector(`#live-roster [data-session="${s.id}"]`),'roster entry');
    document.querySelector(`#live-roster [data-session="${s.id}"]`).click();
    await until(()=>$('live-identity').textContent.includes(s.id),'session selected');
    report('phase','session-ready');
    await sleep(1500); // the script checks the window is mapped before it is closed
    new Image().src='/__workspace-close';
    if(BG_MODE!=='background')return; // the app exits on the accepted close
    await until(async()=>(await shown())>0,'show command arrived',1200);
    // What the user sees after the window comes back.
    const list=await invoke('live_snapshot');
    const now=list.find(x=>x.id===s.id);
    assert(now&&now.status==='running','session still running after hide and show');
    const kept=await output(s);
    assert(kept.r.start_offset===0&&!kept.r.truncated&&kept.text.includes('FIXTURE READY'),'terminal buffer kept from offset 0');
    assert(document.querySelector(`#live-roster [data-session="${s.id}"]`)&&$('live-identity').textContent.includes(s.id),'session still listed and selected');
    const dom=(document.querySelector('#live-terminal .terminal-instance:not([hidden])')?.textContent||'').includes('FIXTURE READY');
    report('terminal_dom_has_output',String(dom));
    await invoke('live_write',{id:s.id,generation:s.generation,data:'/after-show\r'});
    await until(async()=>(await output(s)).text.includes('ACK /after-show'),'terminal accepts input after show');
    report('phase','after-show');
    await sleep(60000); // the shell script ends the app with --quit; a timeout here is a failure by the script's own clock
  }catch(e){report('error',String(e));report('result','fail');}
})();
