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
    await until(()=>$('review-doc').textContent.includes('Why')&&$('review-queue').children.length===2,'queue and draft rendered');
    assert([...$('review-queue').querySelectorAll('button')].every(b=>b.dataset.path.includes('/drafts/')),'only allow-listed drafts are queued');
    assert($('review-current').textContent.endsWith('/pr.md')||$('review-current').textContent.endsWith('/second.md'),'a queued draft is open');
    if(!$('review-doc').querySelector('table')){$('review-queue').querySelector('button[data-path$="/pr.md"]').click();await until(()=>$('review-doc').querySelector('table'),'open pr.md');}
    assert($('review-doc').querySelector('h3')?.textContent==='Why'&&$('review-doc').querySelector('table'),'headings and tables render '+$('review-doc').innerHTML.slice(0,160)+' cur='+$('review-current').textContent);
    assert(!$('review-doc').querySelector('script,img,a,iframe')&&!window.pwned,'draft HTML and links cannot become active content');
    assert($('review-doc').textContent.includes('<script>'),'script text is shown as text');
    if(TEST_PHASE==='first'){
      assert($('review-count').textContent==='2 waiting of 2','both drafts wait in the queue');
      assert($('review-status').dataset.state==='none'&&$('review-save').disabled,'unreviewed and cannot be saved with no checks');
      const boxes=[...document.querySelectorAll('#review-checks input')];
      assert(boxes.length===5,'five checks');
      boxes.slice(0,4).forEach(b=>{b.checked=true;b.dispatchEvent(new Event('change'));});
      assert($('review-save').disabled,'one missing check keeps save disabled');
      boxes[4].checked=true;boxes[4].dispatchEvent(new Event('change'));assert(!$('review-save').disabled,'all checks enable save');
      $('review-form').dispatchEvent(new Event('submit',{cancelable:true}));
      await until(()=>/Next in the queue/.test($('review-notice').textContent),'saved and advanced');
      assert(/not a gate approval/.test($('review-notice').textContent),'notice says it is not a gate approval');
      assert($('review-count').textContent==='1 waiting of 2'&&$('review-current').textContent.endsWith('/second.md'),'queue advanced to the next waiting draft');
      assert($('review-queue').querySelector('button[data-path$="/pr.md"]').dataset.status==='pass','reviewed draft shows PASS (manual review) in the queue');
      report('capture','review-pass');await sleep(1400);
      report('phase','review-first-complete');
    }else{
      assert($('review-count').textContent==='2 waiting of 2','changed draft returns to the queue with the other waiting one');
      assert($('review-queue').querySelector('button[data-path$="/pr.md"]').dataset.status==='stale','changed file is marked changed in the queue, not PASS');
      $('review-queue').querySelector('button[data-path$="/pr.md"]').click();
      await until(()=>$('review-status').dataset.state==='stale','stale draft opened');
      assert($('review-status').textContent.startsWith('STALE'),'changed file shows STALE, not PASS');
      assert($('review-save').disabled,'save needs the checks again after a change');
      report('capture','review-stale');await sleep(1400);
      const boxes=[...document.querySelectorAll('#review-checks input')];boxes.forEach(b=>{b.checked=true;b.dispatchEvent(new Event('change'));});
      $('review-form').dispatchEvent(new Event('submit',{cancelable:true}));
      await until(()=>/Next in the queue/.test($('review-notice').textContent),'re-review of the new text, advance');
      await until(()=>$('review-count').textContent==='1 waiting of 2','one left');
      $('review-queue').querySelector('button[data-path$="/pr.md"]').click();await until(()=>$('review-status').textContent==='PASS (manual review)','re-reviewed text is PASS');
      $('review-clear').click();await until(()=>$('review-status').dataset.state==='none'&&$('review-count').textContent==='2 waiting of 2','label cleared, back in queue');
      report('phase','review-restart-complete');
    }
    {const rail=document.querySelector('.rail'),w0=rail.getBoundingClientRect().width;
     $('rail-toggle').click();await sleep(400);
     assert(document.body.dataset.rail==='collapsed'&&rail.getBoundingClientRect().width<=70,'sidebar collapses to icons');
     assert(getComputedStyle(document.querySelector('#show-review .nav-text')).display==='none'&&$('show-review').getBoundingClientRect().width>30,'labels hidden, icon buttons remain');
     assert($('show-review').title&&$('show-review').getAttribute('aria-label'),'icon buttons keep a name');
     assert(Math.abs(document.querySelector('main').getBoundingClientRect().left-rail.getBoundingClientRect().width)<2,'content follows the collapsed width');
     if(TEST_PHASE==='first'){report('capture','review-collapsed');await sleep(1400);}
     $('rail-toggle').click();await sleep(400);
     assert(rail.getBoundingClientRect().width>=w0-2&&document.body.dataset.rail==='','sidebar expands again');}
    report('result','pass');
  }catch(e){report('error',String(e.message||e));report('result','fail');}
})();
