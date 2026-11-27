'use strict';
(() => {
  const $=id=>document.getElementById(id);
  const invoke=(c,a={})=>AppIPC.invoke(c,a);
  let previewed=null;
  function reset(){previewed=null;$('backup-run').disabled=true;$('backup-run-confirm').hidden=true;}
  async function load(){
    let r;try{r=await invoke('rclone_remotes');}catch(e){$('backup-result').textContent=String(e);return;}
    const sel=$('backup-remote');sel.replaceChildren();reset();
    if(!r.installed){$('backup-result').textContent='rclone was not found. Install it and set up a remote with `rclone config`; the app does not do that for you.';$('backup-dry').disabled=true;return;}
    for(const n of r.remotes){const o=document.createElement('option');o.value=n;o.textContent=n;sel.append(o);}
    $('backup-dry').disabled=!r.remotes.length;
    $('backup-result').textContent=r.remotes.length?'':'rclone has no remotes yet. Run `rclone config` first.';
  }
  async function go(apply){
    const remote=$('backup-remote').value;if(!remote)return;
    $('backup-dry').disabled=true;$('backup-result').textContent=apply?'Copying…':'Checking…';
    try{
      const b=await invoke('rclone_backup',{remote,apply});
      $('backup-result').textContent=b.message+(b.files.length?' Files: '+b.files.join(', '):'');
      if(!apply&&b.ok&&b.count>0){previewed=remote;$('backup-run').disabled=false;}else reset();
      if(apply)reset();
    }catch(e){$('backup-result').textContent=String(e);reset();}
    $('backup-dry').disabled=false;
  }
  $('backup-remote').onchange=reset;
  $('backup-dry').onclick=()=>go(false);
  $('backup-run').onclick=()=>{if(previewed&&previewed===$('backup-remote').value)$('backup-run-confirm').hidden=false;};
  $('backup-run-confirm').onclick=()=>{$('backup-run-confirm').hidden=true;go(true);};
  AppNav.onShow('settings',load);
})();
