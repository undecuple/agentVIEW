// Test-only: seeds synthetic notes and walks the pages so scripts/make-screenshots.sh can capture each one.
(async()=>{
  const $=id=>document.getElementById(id);
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const invoke=(c,a)=>window.__TAURI_INTERNALS__.invoke(c,a);
  async function until(fn,label){for(let i=0;i<200;i++){if(await fn())return;await sleep(100);}throw Error('timeout '+label);}
  const shot=async name=>{await sleep(1200);report('shot',name);await sleep(3500);};
  try{
    await until(()=>document.body.dataset.ready==='true'&&document.body.dataset.preferencesReady==='true','ready');
    const notes=[['Runbooks','Start a release','# Start a release\n\nA checklist kept in this app, not in a repository.\n\n- [ ] Run the test suites\n- [ ] Read the diff\n- [ ] Write the summary\n\n| Step | Owner |\n|---|---|\n| Build | you |\n| Review | a second person |\n\n> Keep notes short and dated.\n'],['Runbooks','Disk cleanup','# Disk cleanup\n\nDelete rebuildable build output first.\n'],['Ideas','Flow view','# Flow view\n\nShow process status only.\n']];
    for(const [shelf,title,body] of notes)await invoke('notes_save',{id:null,shelf,title,body,baseUpdated:null});
    $('show-live').click();await shot('sessions');
    $('show-overview').click();await shot('overview');
    $('show-flow').click();await shot('flow');
    $('show-notes').click();await until(()=>document.querySelectorAll('#notes-tree button, #notes-tree [data-id]').length>0,'notes listed');
    (document.querySelector('#notes-tree [data-id]')||document.querySelector('#notes-tree button')).click();await shot('notes');
    $('show-cc').click();await shot('claude-config');
    $('show-settings').click();await shot('settings');
    report('result','pass');
  }catch(e){report('result','fail:'+e.message);}
})();
