// Test-only: read-only git view against a synthetic repository. Hidden-by-name files and secrets must not reach the page.
(async()=>{
  const $=id=>document.getElementById(id);
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(ok,label)=>{if(!ok)throw Error(label);report('check',label);};
  async function until(fn,label){for(let i=0;i<200;i++){if(await fn())return;await sleep(100);}throw Error('timeout '+label);}
  try{
    await until(()=>document.body.dataset.ready==='true'&&document.body.dataset.preferencesReady==='true','ready');
    $('show-git').click();
    await until(()=>$('git-repo').options.length===1&&$('git-commits').children.length===2,'one repo, two commits');
    assert($('git-branch').textContent==='main','branch name shown');
    assert($('git-files').textContent.includes('a.md')&&$('git-files').textContent.includes('new.txt'),'changed and new files listed');
    assert(!$('git').innerHTML.includes('.env')&&!$('git').innerText.includes('SECRET-VALUE'),'files hidden by name rules are not listed');
    assert($('git-hidden').textContent.includes('1 hidden'),'hidden count shown');
    assert($('git-identity').textContent==='1 differ','one commit with a different e-mail is flagged');
    assert($('git-ab').textContent==='no upstream','no upstream reported');
    let refused=false;try{await invoke_('git_status',{path:'/tmp'});}catch(e){refused=true;}
    assert(refused,'a folder that is not a listed repository is refused');
    report('result','pass');
  }catch(e){report('result','fail:'+e.message);}
  function invoke_(c,a){return window.__TAURI_INTERNALS__.invoke(c,a);}
})();
