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
    { // Built-in role onboarding
      assert($('onb-role').options.length===16&&$('onb-role').querySelectorAll('optgroup').length===3,'sixteen built-in documents in three groups (roles, slash commands, workflow scripts)');
      $('onb-role').value='coordinator';$('onb-role').dispatchEvent(new Event('change'));
      assert($('onb-doc').value.startsWith('# Role: Project coordinator')&&$('onb-doc').value.includes('CLAUDE.md')&&!$('onb-doc').value.includes('{RULES}'),'document names the Claude rules file');
      assert($('onb-prompt').value.includes('coordinator')&&!/[\r\n]/.test($('onb-prompt').value)&&$('onb-prompt').value.length<=4000,'prompt is a single line');
      $('onb-for').value='AGENTS.md';$('onb-for').dispatchEvent(new Event('change'));
      assert($('onb-doc').value.includes('AGENTS.md')&&!$('onb-doc').value.includes('CLAUDE.md'),'document switches to the Codex rules file');
      $('onb-role').value='wf-review';$('onb-role').dispatchEvent(new Event('change'));
      assert($('onb-doc').value.startsWith('export const meta = {')&&$('onb-project').hidden&&$('onb-prompt').value===''&&/\.claude\/workflows\/review-changes\.js/.test($('onb-note').textContent),'a workflow script shows its target path and has no project-save button');
      $('onb-role').value='cmd-handoff';$('onb-role').dispatchEvent(new Event('change'));
      assert($('onb-doc').value.startsWith('---\ndescription:')&&$('onb-doc').value.includes('$ARGUMENTS'),'a slash command has front matter and takes arguments');
      $('onb-role').value='coordinator';$('onb-role').dispatchEvent(new Event('change'));$('onb-for').value='AGENTS.md';$('onb-for').dispatchEvent(new Event('change'));
      $('onb-save').click();
      await until(async()=>(await window.__TAURI_INTERNALS__.invoke('notes_list',{query:'coordinator'})).some(n=>n.shelf==='Onboarding'),'onboarding document saved to the Notes library');
      for(const n of await window.__TAURI_INTERNALS__.invoke('notes_list',{query:'coordinator'}))if(n.shelf==='Onboarding')await window.__TAURI_INTERNALS__.invoke('notes_delete',{id:n.id});
    }
    $('notes-new').click();
    type('note-shelf','Runbooks');type('note-title','Start up');
    type('note-body','## Steps\n\n- [ ] one\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n<script>window.pwned=1</script> [x](javascript:alert(1))');
    await until(()=>$('note-preview').querySelector('table'),'preview renders a table');
    assert($('note-preview').querySelector('h3,h2')&&!$('note-preview').querySelector('script,a,img,iframe')&&!window.pwned&&$('note-preview').textContent.includes('<script>'),'preview is safe: tags shown as text, no links or scripts');
    $('note-save').click();
    await until(()=>$('notes-tree').querySelector('.notes-item')&&$('note-meta').textContent.startsWith('Saved'),'saved and listed');
    assert([...$('notes-tree').querySelectorAll('h4')].some(h=>h.textContent==='Runbooks'),'page is grouped on its shelf');
    type('note-body','# Changed');$('note-save').click();
    await until(()=>$('note-history').options.length===2,'first earlier version kept');
    $('note-history').value=$('note-history').options[1].value;$('note-history').onchange();
    $('note-restore').click();await until(()=>$('note-body').value.includes('## Steps'),'earlier text loaded into editor');
    $('note-body').value='';$('note-body').selectionStart=0;$('note-body').selectionEnd=0;$('note-body').dispatchEvent(new Event('input'));
    document.querySelector('[data-md=b]').click();assert($('note-body').value==='**bold**','toolbar wraps text');
    $('note-save').click();await until(()=>($('notes-status').textContent==='Saved.')&&$('note-body').value==='**bold**','saved the edit');
    type('notes-search','zzz-nothing');await until(()=>$('notes-tree').textContent.includes('No pages match'),'search with no match');
    type('notes-search','');await until(()=>$('notes-tree').querySelector('.notes-item'),'search cleared');
    // Quick switcher
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true}));
    await until(()=>$('qs-dialog').open&&$('qs-list').querySelector('[role=option]'),'Ctrl+K opens the switcher with pages listed');
    type('qs-input','zzz');await until(()=>$('qs-list').textContent.includes('No pages match'),'switcher filter with no match');
    type('qs-input','runb');await until(()=>$('qs-list').querySelector('[role=option][aria-selected=true]'),'switcher filters by shelf and marks one choice');
    $('qs-input').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));
    await until(()=>!$('qs-dialog').open&&$('note-title').value.length>0,'Enter closes the switcher and opens the page');
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
