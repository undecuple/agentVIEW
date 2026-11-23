// Test-only: read-only Claude config view with a synthetic file. Secrets must never reach the page.
(async()=>{
  const $=id=>document.getElementById(id);
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(ok,label)=>{if(!ok)throw Error(label);report('check',label);};
  async function until(fn,label){for(let i=0;i<200;i++){if(await fn())return;await sleep(100);}throw Error('timeout '+label);}
  try{
    await until(()=>document.body.dataset.ready==='true'&&document.body.dataset.preferencesReady==='true','ready');
    $('show-cc').click();
    await until(()=>$('cc-body').children.length===2,'two projects listed');
    const page=$('cc').innerText+$('cc').innerHTML;
    assert(!/SECRET-TOKEN|sk-synthetic|--flag-arg|synthetic-address-value/.test(page),'no env value, argument, header or account detail reaches the page');
    assert($('cc-mcp').textContent.includes('deploy-tool')&&$('cc-body').textContent.includes('has secrets, hidden'),'MCP names shown, secrets only flagged');
    assert($('cc-projects').textContent==='2'&&$('cc-untrusted').textContent==='1'&&$('cc-missing').textContent==='1','counts');
    assert($('cc-note').textContent.includes('account data (not shown)'),'account data noted, not shown');
    report('result','pass');
  }catch(e){report('result','fail:'+e.message);}
})();
