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
    // Settings and inventory tabs: structure only
    window.AppCcTabs.select('settings');
    await until(()=>$('cs-allow').textContent==='2','settings loaded');
    const sp=$('cc').innerText+$('cc').innerHTML;
    assert(!/SECRET-HOOK-ARG|SECRET-ENV-VALUE|SECRET-HELPER|abcdefghijklmnopqrstuvwxyz012345/.test(sp),'no hook argument, env value, helper command or token reaches the settings tab');
    assert($('cs-hook-rows').textContent.includes('notify-tool')&&$('cs-hook-rows').textContent.includes('permission_prompt'),'hook shows program name and matcher');
    assert($('cs-other').textContent.includes('API_TOKEN')&&$('cs-other').textContent.includes('apiKeyHelper'),'env names and helper flag shown, values not');
    assert($('cs-mode').textContent.includes('acceptEdits'),'default permission mode shown');
    window.AppCcTabs.select('skills');
    await until(()=>$('ci-rows').children.length===3,'three inventory entries');
    const ip=$('cc').innerText;
    assert(!/SECRET-BODY/.test(ip)&&ip.includes('Summarises synthetic text')&&ip.includes('Deploy the synthetic thing')&&ip.includes('model: synthetic-small'),'inventory shows names and descriptions, never bodies');
    window.AppCcTabs.select('memory');
    await until(()=>$('cm-select').options.length>=2&&$('cm-text').textContent.includes('Be brief'),'memory list and first file');
    $('cm-select').value=[...$('cm-select').options].find(o=>o.textContent.includes('pref.md')).value;$('cm-select').dispatchEvent(new Event('change'));
    await until(()=>$('cm-text').textContent.includes('synthetic data'),'second memory file shown');
    window.AppCcTabs.select('history');
    await until(()=>$('ch-rows').children.length===1,'one past session');
    const hp=$('cc').innerText;
    assert(hp.includes('claude --resume 11111111-2222-3333-4444-555555555555')&&hp.includes('-work-demo'),'history shows resume command and project');
    assert(!/SECRET-CONVERSATION-TEXT/.test(hp),'conversation text never opened');
    report('result','pass');
  }catch(e){report('result','fail:'+e.message);}
})();
