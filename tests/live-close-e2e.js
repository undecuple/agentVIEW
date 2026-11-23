// Test-only: actual native CloseRequested while a dedicated Codex helper is stalled.
(async()=>{
  const invoke=(c,a={})=>window.__TAURI_INTERNALS__.invoke(c,a);
  const report=(key,value)=>{new Image().src=`/__workspace-report?${key}=${encodeURIComponent(value)}`;};
  async function until(fn){for(let i=0;i<150;i++){if(await fn())return;await new Promise(r=>setTimeout(r,100));}throw Error('stalled helper never became ready');}
  try{
    await invoke('live_probe',{provider:'claude'});await invoke('live_probe',{provider:'codex'});
    const claude=await invoke('live_create',{provider:'claude',label:'close-control Claude',projectDir:`${PROJECT_ROOT}/alpha`});
    const codex=await invoke('live_create',{provider:'codex',label:'cancel-bootstrap Codex',projectDir:`${PROJECT_ROOT}/beta`});
    await invoke('live_start',{id:claude.id});
    invoke('live_start',{id:codex.id}).catch(()=>{});
    await until(()=>new Promise(resolve=>{const script=document.createElement('script');script.src='/__workspace-cancel-status.js';script.onload=()=>{script.remove();resolve(window.__cancelReady);};document.head.append(script);}));
    report('phase','native-close-ready');
    new Image().src='/__workspace-close';
  }catch(e){report('error',String(e));report('result','fail');}
})();
