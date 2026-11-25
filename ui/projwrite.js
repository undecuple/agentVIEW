'use strict';
// Consent dialog for writing one of the app's own documents into a registered session's project folder.
// Flow: choose project and file name, Preview (nothing written, shows the existing file), then a two-step confirm. The Rust side re-checks everything.
window.ProjWrite=(()=>{
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  let content='',pv=null,armed=false,seq=0,opener=null;
  const reset=()=>{seq++;$('pw-cancel').textContent='Cancel';pv=null;armed=false;$('pw-write').disabled=true;$('pw-write').textContent='Write file';$('pw-current').hidden=true;$('pw-current').value='';$('pw-new-wrap').hidden=true;};
  async function open({content:c,name,names}){
    opener=document.activeElement;content=c;reset();$('pw-info').textContent='Loading your registered sessions…';$('pw-new').value=c;
    const sel=$('pw-session');sel.replaceChildren();
    let list=[];try{list=await invoke('live_snapshot');}catch(e){$('pw-info').textContent=String(e);}
    for(const s of list){const o=document.createElement('option');o.value=s.id;o.textContent=`${s.label||s.id} · ${s.project_dir}`;sel.append(o);}
    const nm=$('pw-name');nm.replaceChildren();for(const n of names){const o=document.createElement('option');o.value=n;o.textContent=n;nm.append(o);}
    if(names.includes(name))nm.value=name;
    $('pw-preview').disabled=!list.length;if(!list.length)$('pw-info').textContent='Register a session for the project first (Sessions page); the app writes only into a registered session\'s folder.';
    if(list.length)$('pw-info').textContent='Choose the project and file, then press Preview. Nothing is written yet.';
    $('pw-dialog').showModal();$('pw-session').focus();
  }
  $('pw-dialog').addEventListener('close',()=>{if(opener&&opener.isConnected)opener.focus();});
  $('pw-session').onchange=reset;$('pw-name').onchange=reset;$('pw-backup').onchange=reset;
  $('pw-preview').onclick=async()=>{
    reset();const mine=seq;$('pw-info').textContent='Checking…';
    try{
      const got=await invoke('project_file_preview',{id:$('pw-session').value,name:$('pw-name').value,content});
      if(mine!==seq)return;pv=got;$('pw-new-wrap').hidden=false;
      if(pv.exists&&pv.same){$('pw-info').textContent=`${pv.target} already has exactly this text. Nothing to do.`;return;}
      if(pv.exists){$('pw-info').textContent=`${pv.target} exists (${pv.current_size} bytes) and will be replaced. Its current text is shown below${pv.current_size>65536?' (first 64 KB; secrets are masked)':' (secrets are masked)'}, and the new text that will replace it is under it. ${$('pw-backup').checked?'It will be backed up first.':'No backup will be made.'}`;$('pw-current').value=pv.current_text;$('pw-current').hidden=false;}
      else $('pw-info').textContent=`${pv.target} does not exist yet and will be created with the text shown below.`;
      $('pw-write').textContent=pv.exists?'Replace file':'Create file';$('pw-write').disabled=false;
    }catch(e){if(mine===seq)$('pw-info').textContent=window.friendly('The preview failed. Nothing was written',e);}
  };
  $('pw-write').onclick=async()=>{
    if(!pv)return;
    if(!armed){armed=true;$('pw-write').textContent=`Confirm: write ${pv.target}`;$('pw-info').textContent=`Press the button again to ${pv.exists?'replace':'create'} ${pv.target}. Nothing has been written yet. Press Cancel to stop.`;return;}
    $('pw-write').disabled=true;
    try{
      const w=await invoke('project_file_write',{id:$('pw-session').value,name:$('pw-name').value,content,expectedHash:pv.current_hash,backup:$('pw-backup').checked,confirm:true});
      $('pw-info').textContent=`Written: ${w.target}.${w.backup?` The old file was backed up as ${w.backup} in the app's backups folder.`:''}`;
    }catch(e){$('pw-info').textContent=window.friendly('The write did not happen. Your file is unchanged unless the message says otherwise',e);}
    pv=null;armed=false;$('pw-write').textContent='Write file';$('pw-cancel').textContent='Close';$('pw-cancel').focus();
  };
  $('pw-cancel').onclick=()=>$('pw-dialog').close();
  return {open};
})();
