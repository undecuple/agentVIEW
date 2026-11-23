'use strict';
(() => {
  const $=id=>document.getElementById(id),invoke=(cmd,args={})=>AppIPC.invoke(cmd,args);
  const LABELS={tests_run:'Test commands were run, and results are stated as run or not run',wording_true:'Wording is plain and every claim is true',overlap_listed:'Overlap with open PRs on the same files is listed',evidence_stated:'Evidence says what was and was not run',no_private_names:'No private names, paths or hosts in the text'};
  let view=null,files=[],loading=false,current='';
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
  const STATUS={pending:'Waiting',stale:'Changed',pass:'PASS (manual review)'};
  function drawQueue(){
    const ol=$('review-queue');ol.replaceChildren();
    const waiting=files.filter(f=>f.status!=='pass').length;
    $('review-count').textContent=files.length?`${waiting} waiting of ${files.length}`:'Queue is empty';
    $('review-next').disabled=!waiting;
    for(const f of files){
      const li=document.createElement('li');const b=document.createElement('button');b.type='button';b.dataset.status=f.status;b.dataset.path=f.path;
      b.setAttribute('aria-pressed',String(f.path===current));
      const n=document.createElement('span');n.className='q-name';n.textContent=f.name;
      const s=document.createElement('span');s.className='q-status';s.textContent=STATUS[f.status]||f.status;
      b.append(n,s);b.onclick=()=>open(f.path);li.append(b);ol.append(li);
    }
  }
  async function open(path){
    current=path;drawQueue();
    $('review-current').textContent=path?files.find(f=>f.path===path)?.name||'':'';
    if(!path){view=null;$('review-doc').textContent='';$('review-status').textContent='';sync();return;}
    loading=true;sync();
    try{render(await invoke('review_load',{path}));notice('');}catch(e){view=null;$('review-doc').textContent='';notice(String(e));}
    finally{loading=false;sync();}
  }
  async function loadFiles(keep){
    try{files=await invoke('review_files');}catch(e){notice(String(e));return;}
    const pick=files.find(f=>f.path===keep)||files.find(f=>f.status!=='pass')||files[0];
    current=pick?pick.path:'';drawQueue();open(current);
  }
  function nextWaiting(after){
    const i=files.findIndex(f=>f.path===after);
    const order=[...files.slice(i+1),...files.slice(0,Math.max(i,0))];
    return order.find(f=>f.status!=='pass'&&f.path!==after);
  }
  async function refreshAfter(path,message){
    files=await invoke('review_files');
    const next=nextWaiting(path);
    if(next){current=next.path;drawQueue();await open(next.path);notice(message+' Next in the queue: '+next.name);}
    else{drawQueue();await open(path);notice(message+' The queue is empty.');}
  }
  $('review-reload').onclick=()=>loadFiles(current);
  $('review-next').onclick=()=>{const n=nextWaiting(current)||files.find(f=>f.status!=='pass');if(n)open(n.path);};
  $('close-review').onclick=()=>AppNav.navigate('sessions');
  $('review-form').onsubmit=async e=>{
    e.preventDefault();if(!view||loading)return;loading=true;sync();
    try{const checks=[...document.querySelectorAll('#review-checks input:checked')].map(i=>i.dataset.check);
      const done=view.path;render(await invoke('review_mark',{path:done,sha256:view.sha256,checks}));await refreshAfter(done,'Saved: PASS (manual review). This is your note, not a gate approval.');}
    catch(err){notice(String(err));}finally{loading=false;sync();}
  };
  $('review-clear').onclick=async()=>{if(!view||loading)return;loading=true;sync();try{const p=view.path;render(await invoke('review_clear',{path:p}));files=await invoke('review_files');drawQueue();notice('Label cleared.');}catch(err){notice(String(err));}finally{loading=false;sync();}};
  window.addEventListener('agentview:navigate',e=>{if(e.detail==='review')loadFiles(current);});
})();
