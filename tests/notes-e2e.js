// Test-only: notes library and export/import bundle in the real UI. Synthetic text only.
(async()=>{
  const $=id=>document.getElementById(id);
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(ok,label)=>{if(!ok)throw Error(label);report('check',label);};
  async function until(fn,label){for(let i=0;i<200;i++){if(await fn())return;await sleep(100);}throw Error(`timeout ${label}; status=${$('notes-status')?.textContent} bundle=${$('bundle-summary')?.textContent}`);}
  const type=(id,v)=>{$(id).value=v;$(id).dispatchEvent(new Event('input'));};
  try{
    await until(()=>document.body.dataset.ready==='true'&&document.body.dataset.preferencesReady==='true','ready');
    $('show-notes').click();
    await until(()=>!$('notes').hidden,'notes page');
    $('notes-new').click();
    type('note-shelf','Runbooks');type('note-title','Start up');
    type('note-body','## Steps\n\n- [ ] one\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n<script>window.pwned=1</script> [x](javascript:alert(1))');
    await until(()=>$('note-preview').querySelector('table'),'preview renders a table');
    assert($('note-preview').querySelector('h3,h2')&&!$('note-preview').querySelector('script,a,img,iframe')&&!window.pwned&&$('note-preview').textContent.includes('<script>'),'preview is safe: tags shown as text, no links or scripts');
    $('note-save').click();
    await until(()=>$('notes-tree').querySelector('.notes-item')&&$('note-meta').textContent.startsWith('Saved'),'saved and listed');
    assert($('notes-tree').querySelector('h4').textContent==='Runbooks','page is grouped on its shelf');
    type('note-body','# Changed');$('note-save').click();
    await until(()=>$('note-history').options.length===2,'first earlier version kept');
    $('note-history').value=$('note-history').options[1].value;$('note-history').onchange();
    $('note-restore').click();await until(()=>$('note-body').value.includes('## Steps'),'earlier text loaded into editor');
    $('note-body').value='';$('note-body').selectionStart=0;$('note-body').selectionEnd=0;$('note-body').dispatchEvent(new Event('input'));
    document.querySelector('[data-md=b]').click();assert($('note-body').value==='**bold**','toolbar wraps text');
    $('note-save').click();await until(()=>($('notes-status').textContent==='Saved.')&&$('note-body').value==='**bold**','saved the edit');
    type('notes-search','zzz-nothing');await until(()=>$('notes-tree').textContent.includes('No pages match'),'search with no match');
    type('notes-search','');await until(()=>$('notes-tree').querySelector('.notes-item'),'search cleared');
    // Bundle: export, check, import adds a renamed copy and never overwrites
    $('show-settings').click();await sleep(200);
    $('bundle-export').click();await until(()=>$('bundle-summary').textContent.includes('Bundle saved'),'bundle exported');
    assert($('bundle-pick').options.length===2,'bundle listed');
    $('bundle-preview').click();await until(()=>/1 page\(s\) on 1 shelf/.test($('bundle-summary').textContent),'bundle checked');
    assert(!$('bundle-import').disabled,'import allowed only after the check');
    $('bundle-import').click();await until(()=>$('bundle-summary').textContent.includes('Imported 1 page'),'bundle imported');
    assert($('bundle-summary').textContent.includes('renamed')&&$('bundle-summary').textContent.includes('preferences unchanged'),'clash renamed and preferences untouched by default');
    $('show-notes').click();await until(()=>[...document.querySelectorAll('.notes-item')].some(b=>b.textContent.includes('(imported)')),'imported copy listed');
    document.querySelector('.notes-item').click();await sleep(300);
    $('note-delete').click();assert(!$('note-delete-confirm').hidden,'delete needs a second click');
    $('note-delete-confirm').click();await until(()=>$('notes-editor').hidden,'page deleted');
    report('result','pass');
  }catch(e){report('result','fail:'+e.message);}
})();
