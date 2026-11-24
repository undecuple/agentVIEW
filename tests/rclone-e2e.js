// Test-only: rclone backup page against a fake rclone program. No real remote is contacted.
(async()=>{
  const $=id=>document.getElementById(id);
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(ok,label)=>{if(!ok)throw Error(label);report('check',label);};
  const invoke=(c,a)=>window.__TAURI_INTERNALS__.invoke(c,a);
  async function until(fn,label){for(let i=0;i<200;i++){if(await fn())return;await sleep(100);}throw Error('timeout '+label);}
  try{
    await until(()=>document.body.dataset.ready==='true'&&document.body.dataset.preferencesReady==='true','ready');
    await invoke('bundle_export');
    $('show-settings').click();
    await until(()=>$('backup-remote').options.length===2,'two remote names listed');
    assert([...$('backup-remote').options].map(o=>o.value).join()==='mydrive,other','only remote names shown');
    assert($('backup-run').disabled,'copy is disabled before a dry run');
    let early=false;try{await invoke('rclone_backup',{remote:'mydrive',apply:true});}catch(e){early=true;}
    assert(early,'a real copy without a dry run is refused');
    let bad=false;try{await invoke('rclone_backup',{remote:'--delete-after',apply:false});}catch(e){bad=true;}
    assert(bad,'a name that is not a listed remote is refused');
    $('backup-dry').click();
    await until(()=>$('backup-result').textContent.includes('would be copied'),'dry run result');
    assert($('backup-result').textContent.includes('agentview-bundle-'),'dry run lists the bundle file name');
    assert(!$('backup-run').disabled,'copy enabled after a dry run');
    $('backup-run').click();
    assert(!$('backup-run-confirm').hidden,'a second confirming click is required');
    $('backup-run-confirm').click();
    await until(()=>$('backup-result').textContent.includes('Copied 1 new file'),'copy result');
    assert($('backup-run').disabled,'copy disabled again after a real copy');
    let twice=false;try{await invoke('rclone_backup',{remote:'mydrive',apply:true});}catch(e){twice=true;}
    assert(twice,'a second real copy needs a new dry run');
    report('result','pass');
  }catch(e){report('result','fail:'+e.message);}
})();
