// Test-only: manual review of a synthetic draft in the real UI. No provider process.
(async()=>{
  const $=id=>document.getElementById(id);
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(ok,label)=>{if(!ok)throw Error(label);report('check',label);};
  async function until(fn,label){for(let i=0;i<200;i++){if(await fn())return;await sleep(100);}throw Error(`timeout ${label}; notice=${$('review-notice')?.textContent}`);}
  try{
    await until(()=>document.body.dataset.ready==='true'&&document.body.dataset.preferencesReady==='true','ready');
    $('show-review').click();
    await until(()=>$('review-doc').textContent.includes('Why'),'draft rendered');
    assert($('review-file').options.length===1&&$('review-file').value.endsWith('/pr.md'),'only the allow-listed draft is offered');
    assert($('review-doc').querySelector('h3')?.textContent==='Why'&&$('review-doc').querySelector('table'),'headings and tables render');
    assert(!$('review-doc').querySelector('script,img,a,iframe')&&!window.pwned,'draft HTML and links cannot become active content');
    assert($('review-doc').textContent.includes('<script>'),'script text is shown as text');
    if(TEST_PHASE==='first'){
      assert($('review-status').dataset.state==='none'&&$('review-save').disabled,'unreviewed and cannot be saved with no checks');
      const boxes=[...document.querySelectorAll('#review-checks input')];
      assert(boxes.length===5,'five checks');
      boxes.slice(0,4).forEach(b=>{b.checked=true;b.dispatchEvent(new Event('change'));});
      assert($('review-save').disabled,'one missing check keeps save disabled');
      boxes[4].checked=true;boxes[4].dispatchEvent(new Event('change'));assert(!$('review-save').disabled,'all checks enable save');
      $('review-form').dispatchEvent(new Event('submit',{cancelable:true}));
      await until(()=>$('review-status').textContent==='PASS (manual review)','PASS (manual review) saved');
      assert(/not a gate approval/.test($('review-notice').textContent),'notice says it is not a gate approval');
      report('capture','review-pass');await sleep(1400);
      report('phase','review-first-complete');
    }else{
      assert($('review-status').dataset.state==='stale'&&$('review-status').textContent.startsWith('STALE'),'changed file shows STALE, not PASS');
      assert($('review-save').disabled,'save needs the checks again after a change');
      report('capture','review-stale');await sleep(1400);
      const boxes=[...document.querySelectorAll('#review-checks input')];boxes.forEach(b=>{b.checked=true;b.dispatchEvent(new Event('change'));});
      $('review-form').dispatchEvent(new Event('submit',{cancelable:true}));
      await until(()=>$('review-status').textContent==='PASS (manual review)','re-review of the new text');
      $('review-clear').click();await until(()=>$('review-status').dataset.state==='none','label cleared');
      report('phase','review-restart-complete');
    }
    report('result','pass');
  }catch(e){report('error',String(e.message||e));report('result','fail');}
})();
