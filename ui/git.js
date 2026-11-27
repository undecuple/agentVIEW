'use strict';
(() => {
  const $=id=>document.getElementById(id);
  const cell=(tr,t,cls)=>{const td=document.createElement('td');td.textContent=t;if(cls)td.className=cls;tr.append(td);return td;};
  async function show(){
    const path=$('git-repo').value;if(!path)return;let v;
    try{v=await AppIPC.invoke('git_status',{path});}catch(e){$('git-note').textContent=String(e);return;}
    $('git-note').textContent=v.note;
    $('git-branch').textContent=v.detached?'detached HEAD':v.branch;
    $('git-ab').textContent=v.upstream?`${v.ahead??0} / ${v.behind??0}`:'no upstream';
    $('git-changed').textContent=String(v.changed_total);
    $('git-identity').textContent=v.local_email?(v.identity_mismatch?`${v.identity_mismatch} differ`:'matches'):'not set here';
    $('git-hidden').textContent=v.hidden_by_policy?`${v.hidden_by_policy} hidden by name rules`:'';
    $('git-who').textContent=v.local_email?`repo identity: ${v.local_name||'?'} <${v.local_email}>`:'no repo-local identity';
    const files=$('git-files');files.replaceChildren();
    for(const f of v.changed){const tr=document.createElement('tr');cell(tr,f.state==='??'?'new':f.state,'mono');cell(tr,f.path,'mono');files.append(tr);}
    if(v.changed_total>v.changed.length){const tr=document.createElement('tr');cell(tr,'…');cell(tr,`${v.changed_total-v.changed.length} more not listed`);files.append(tr);}
    const commits=$('git-commits');commits.replaceChildren();
    for(const c of v.commits){const tr=document.createElement('tr');cell(tr,c.id,'mono');cell(tr,c.date);cell(tr,c.author);cell(tr,c.subject);if(!c.identity_ok)tr.title='Author or committer e-mail differs from the repo identity';commits.append(tr);}
  }
  async function load(){
    let repos;
    try{repos=await AppIPC.invoke('git_repos');}catch(e){$('git-note').textContent=String(e);return;}
    const sel=$('git-repo'),keep=sel.value;sel.replaceChildren();
    for(const r of repos){const o=document.createElement('option');o.value=r.path;o.textContent=r.name;sel.append(o);}
    if(!repos.length){$('git-note').textContent='None of the shared folders is inside a git repository. Share a project folder on the Documents page to see it here.';return;}
    if(repos.some(r=>r.path===keep))sel.value=keep;
    show();
  }
  $('git-repo').onchange=show;$('git-reload').onclick=load;AppNav.onShow('git',load);
})();
