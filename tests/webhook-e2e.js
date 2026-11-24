// Test-only: several webhook destinations against a fake curl program. Addresses must never come back to the page.
(async()=>{
  const $=id=>document.getElementById(id);
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(ok,label)=>{if(!ok)throw Error(label);report('check',label);};
  const invoke=(c,a)=>window.__TAURI_INTERNALS__.invoke(c,a);
  async function until(fn,label){for(let i=0;i<200;i++){if(await fn())return;await sleep(100);}throw Error('timeout '+label);}
  const rows=()=>[...$('hook-rows').children];
  const add=async(name,url)=>{$('hook-name').value=name;$('hook-url').value=url;$('hook-save').click();};
  const SECRET='SECRET-TOKEN-123',SECRET2='SECRET-TOKEN-456';
  try{
    await until(()=>document.body.dataset.ready==='true'&&document.body.dataset.preferencesReady==='true','ready');
    $('show-settings').click();
    await until(()=>$('hook-state').textContent.includes('No destinations yet'),'status loaded');
    for(const bad of ['http://hooks.example.test/x','https://localhost/x','https://10.1.2.3/x','https://hooks.example.test/x y']){
      await add('Bad',bad);
      await until(()=>$('hook-state').textContent.length>0&&!$('hook-state').textContent.includes('destination(s)'),'refusal '+bad);
      assert(rows().length===0,'refused: '+bad);
    }
    await add('Phone','https://hooks.example.test/api/'+SECRET);
    await until(()=>rows().length===1,'first saved');
    $('hook-ev-finished').checked=true;
    await add('Team','https://chat.example.test/api/'+SECRET2);
    await until(()=>rows().length===2,'second saved');
    assert(rows()[0].textContent.includes('Phone')&&rows()[0].textContent.includes('hooks.example.test')&&rows()[0].textContent.includes('Quiet, Error'),'first row shows name, host and events');
    assert(rows()[1].textContent.includes('Quiet, Error, Finished'),'second row has its own events');
    const page=$('settings').innerHTML+$('settings').innerText+JSON.stringify(await invoke('webhook_status',{}));
    assert(!page.includes(SECRET)&&!page.includes(SECRET2)&&$('hook-url').value==='','addresses are not shown or returned');
    // Edit the first one without a new address
    rows()[0].querySelectorAll('button')[1].click();
    assert($('hook-name').value==='Phone'&&$('hook-url').value==='','edit fills the name, never the address');
    $('hook-name').value='Phone 2';$('hook-ev-quiet').checked=false;$('hook-save').click();
    await until(()=>rows()[0].textContent.includes('Phone 2')&&rows()[0].textContent.includes('Error')&&!rows()[0].textContent.includes('Quiet'),'edited');
    rows()[0].querySelectorAll('button')[0].click();
    await until(()=>$('hook-state').textContent.includes('Test message sent to Phone 2'),'test message sent');
    // Remove needs a second click
    rows()[1].querySelectorAll('button')[2].click();
    await until(()=>rows()[1].textContent.includes('Confirm remove'),'second click offered');
    assert(rows().length===2,'still there after the first click');
    [...rows()[1].querySelectorAll('button')].find(b=>b.textContent==='Confirm remove').click();
    await until(()=>rows().length===1,'removed');
    let t=false;try{await invoke('webhook_test',{id:99999});}catch(e){t=true;}
    assert(t,'test of a missing destination is refused');
    report('result','pass');
  }catch(e){report('result','fail:'+e.message);}
})();
