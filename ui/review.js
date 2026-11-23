'use strict';
(() => {
  const $=id=>document.getElementById(id),invoke=(cmd,args={})=>AppIPC.invoke(cmd,args);
  const LABELS={tests_run:'Test commands were run, and results are stated as run or not run',wording_true:'Wording is plain and every claim is true',overlap_listed:'Overlap with open PRs on the same files is listed',evidence_stated:'Evidence says what was and was not run',no_private_names:'No private names, paths or hosts in the text'};
  let view=null,files=[],loading=false;
  const notice=t=>{$('review-notice').textContent=t;};
  function boxes(required){
    const box=$('review-checks');box.replaceChildren();
    for(const key of required){const l=document.createElement('label');l.className='check-label';const i=document.createElement('input');i.type='checkbox';i.dataset.check=key;i.onchange=sync;l.append(i,' '+(LABELS[key]||key));box.append(l);}
  }
  function sync(){
    const all=[...document.querySelectorAll('#review-checks input')].every(i=>i.checked);
    $('review-save').disabled=!view||!all||loading;$('review-clear').disabled=!view||!view.review||loading;
  }
  function render(v){
    view=v;const status=$('review-status');
    $('review-doc').innerHTML=v.html;
    if(!v.review){status.textContent='Not reviewed';status.dataset.state='none';}
    else if(v.stale){status.textContent='STALE: file changed since your review';status.dataset.state='stale';}
    else{status.textContent='PASS (manual review)';status.dataset.state='pass';}
    $('review-meta').textContent=v.review?`${v.stale?'Last reviewed':'Reviewed'} ${v.review.reviewed_at.replace('T',' ').slice(0,16)} UTC · text hash ${v.review.sha256.slice(0,12)}`:`Text hash ${v.sha256.slice(0,12)}`;
    boxes(v.required_checks);sync();
  }
  async function open(path){
    if(!path){view=null;$('review-doc').textContent='';$('review-status').textContent='';sync();return;}
    loading=true;sync();
    try{render(await invoke('review_load',{path}));notice('');}catch(e){view=null;$('review-doc').textContent='';notice(String(e));}
    finally{loading=false;sync();}
  }
  async function loadFiles(){
    try{files=await invoke('review_files');}catch(e){notice(String(e));return;}
    const sel=$('review-file'),keep=sel.value;sel.replaceChildren();
    if(!files.length){const o=document.createElement('option');o.value='';o.textContent='No allow-listed Markdown or text files';sel.append(o);open('');return;}
    for(const f of files){const o=document.createElement('option');o.value=f.path;o.textContent=f.name;sel.append(o);}
    sel.value=files.some(f=>f.path===keep)?keep:files[0].path;open(sel.value);
  }
  $('review-file').onchange=()=>open($('review-file').value);
  $('review-reload').onclick=loadFiles;
  $('close-review').onclick=()=>AppNav.navigate('sessions');
  $('review-form').onsubmit=async e=>{
    e.preventDefault();if(!view||loading)return;loading=true;sync();
    try{const checks=[...document.querySelectorAll('#review-checks input:checked')].map(i=>i.dataset.check);
      render(await invoke('review_mark',{path:view.path,sha256:view.sha256,checks}));notice('Saved: PASS (manual review). This is your note; it is not a gate approval.');}
    catch(err){notice(String(err));}finally{loading=false;sync();}
  };
  $('review-clear').onclick=async()=>{if(!view||loading)return;loading=true;sync();try{render(await invoke('review_clear',{path:view.path}));notice('Label cleared.');}catch(err){notice(String(err));}finally{loading=false;sync();}};
  window.addEventListener('agentview:navigate',e=>{if(e.detail==='review')loadFiles();});
})();
