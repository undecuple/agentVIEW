'use strict';
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  let list=[],note=null,dirty=false,timer=0,loadedOnce=false;
  const status=t=>{$('notes-status').textContent=t;};
  const err=e=>status(String(e&&e.message||e));
  async function refresh(){
    try{list=await invoke('notes_list',{query:$('notes-search').value});}catch(e){return err(e);}
    const tree=$('notes-tree');tree.replaceChildren();
    if(!list.length){const p=document.createElement('p');p.className='muted';p.textContent=$('notes-search').value?'No pages match.':'No pages yet. Choose New page.';tree.append(p);return;}
    let shelf=null,group=null;
    for(const n of list){
      if(n.shelf!==shelf){shelf=n.shelf;const h=document.createElement('h4');h.textContent=shelf;tree.append(h);group=document.createElement('div');group.className='notes-group';tree.append(group);}
      const b=document.createElement('button');b.type='button';b.className='notes-item';b.textContent=n.title;b.dataset.id=n.id;if(note&&note.id===n.id)b.dataset.active='true';b.onclick=()=>open(n.id);group.append(b);
    }
  }
  function show(n){
    note=n;dirty=false;$('notes-editor').hidden=false;
    $('note-shelf').value=n.shelf;$('note-title').value=n.title;$('note-body').value=n.body;
    const sel=$('note-history');sel.replaceChildren(new Option('Earlier versions',''));
    for(const r of n.revisions||[])sel.append(new Option(`${r.saved_at.replace('T',' ').slice(0,16)} UTC · ${r.size} bytes`,String(r.id)));
    $('note-restore').disabled=true;$('note-meta').textContent=n.id?`Saved ${n.updated.replace('T',' ').slice(0,16)} UTC`:'Not saved yet';
    $('note-delete').hidden=!n.id;resetDelete();preview();refresh();
  }
  async function open(id){
    if(dirty&&!confirm('Discard unsaved changes to this page?'))return;
    try{show(await invoke('notes_get',{id}));status('');}catch(e){err(e);}
  }
  async function preview(){
    const text=$('note-body').value;
    try{$('note-preview').innerHTML=await invoke('notes_render',{text});}catch(e){$('note-preview').textContent=String(e);}
  }
  function schedule(){dirty=true;clearTimeout(timer);timer=setTimeout(preview,250);}
  async function save(){
    try{
      const n=await invoke('notes_save',{id:note&&note.id||null,shelf:$('note-shelf').value,title:$('note-title').value,body:$('note-body').value,baseUpdated:note&&note.id?note.updated:null});
      show(n);status('Saved.');
    }catch(e){err(e);}
  }
  function resetDelete(){$('note-delete-confirm').hidden=true;if(note&&note.id)$('note-delete').hidden=false;}
  function wrap(kind){
    const t=$('note-body'),s=t.selectionStart,e=t.selectionEnd,sel=t.value.slice(s,e);
    const w=(a,b,ph)=>{const m=sel||ph;t.setRangeText(a+m+b,s,e,'end');};
    const line=(p,ph)=>{const m=sel||ph;t.setRangeText(m.split('\n').map(l=>p+l).join('\n'),s,e,'end');};
    ({b:()=>w('**','**','bold'),i:()=>w('*','*','italic'),code:()=>w('`','`','code'),h:()=>line('## ','Heading'),ul:()=>line('- ','item'),task:()=>line('- [ ] ','task'),quote:()=>line('> ','quote'),
      link:()=>w('[','](https://)','text'),table:()=>t.setRangeText('| Column | Column |\n| --- | --- |\n| cell | cell |\n',s,e,'end')})[kind]();
    t.focus();schedule();
  }
  $('notes-search').oninput=()=>{clearTimeout($('notes-search')._t);$('notes-search')._t=setTimeout(refresh,200);};
  $('notes-new').onclick=()=>{if(dirty&&!confirm('Discard unsaved changes to this page?'))return;show({id:0,shelf:$('note-shelf').value||'General',title:'',body:'',updated:'',revisions:[]});$('note-title').focus();};
  for(const id of ['note-body','note-title','note-shelf'])$(id).addEventListener('input',schedule);
  $('note-body').addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&e.key==='s'){e.preventDefault();save();}});
  $('note-save').onclick=save;
  $('note-delete').onclick=()=>{$('note-delete').hidden=true;$('note-delete-confirm').hidden=false;};
  $('note-delete-confirm').onclick=async()=>{try{await invoke('notes_delete',{id:note.id});note=null;dirty=false;$('notes-editor').hidden=true;status('Page deleted.');refresh();}catch(e){err(e);}};
  $('note-history').onchange=()=>{$('note-restore').disabled=!$('note-history').value;};
  $('note-restore').onclick=async()=>{try{$('note-body').value=await invoke('notes_revision',{id:note.id,revision:Number($('note-history').value)});schedule();status('Earlier text loaded. Save to keep it.');}catch(e){err(e);}};
  $('note-view').onchange=()=>{$('md-panes').dataset.view=$('note-view').value;};
  for(const b of document.querySelectorAll('[data-md]'))b.onclick=()=>wrap(b.dataset.md);
  window.addEventListener('agentview:navigate',e=>{if(e.detail==='notes'){refresh();loadedOnce=true;}});
  $('show-notes').addEventListener('click',()=>refresh());
  // Export / import bundle (Settings)
  const bs=t=>{$('bundle-summary').textContent=t;};
  async function bundles(){
    try{const l=await invoke('bundle_list');const sel=$('bundle-pick');sel.replaceChildren(new Option('Choose a bundle',''));for(const f of l)sel.append(new Option(`${f.name} (${Math.round(f.size/1024)} KB)`,f.name));}catch(e){bs(String(e));}
  }
  $('bundle-export').onclick=async()=>{try{const f=await invoke('bundle_export');bs(`Bundle saved in the exports folder: ${f.name}`);await bundles();$('bundle-pick').value=f.name;$('bundle-pick').onchange();}catch(e){bs(String(e));}};
  $('bundle-pick').onchange=()=>{$('bundle-preview').disabled=!$('bundle-pick').value;$('bundle-import').disabled=true;};
  $('bundle-preview').onclick=async()=>{try{const s=await invoke('bundle_preview',{name:$('bundle-pick').value});bs(`Valid bundle from app ${s.app_version}: ${s.pages} page(s) on ${s.shelves} shelf(s); preferences included (theme ${s.theme}). Nothing has been imported yet.`);$('bundle-import').disabled=false;}catch(e){bs(String(e));$('bundle-import').disabled=true;}};
  $('bundle-import').onclick=async()=>{try{const r=await invoke('bundle_import',{name:$('bundle-pick').value,includeSettings:$('bundle-settings').checked});bs(`Imported ${r.pages_added} page(s)${r.pages_renamed?` (${r.pages_renamed} renamed with "(imported)")`:''}${r.settings_applied?'; preferences replaced':'; preferences unchanged'}.`);$('bundle-import').disabled=true;refresh();if(r.settings_applied)location.reload();}catch(e){bs(String(e));}};
  $('show-settings').addEventListener('click',bundles);
  bundles();
})();
