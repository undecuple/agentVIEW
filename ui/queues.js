'use strict';
// Queues card (read-only). A scheduler or adapter script writes queues/<name>.jsonl; this page only reads and draws it. Hidden until a queue has something in it.
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  let timer=null;
  const dur=s=>s<90?`${Math.round(s)} s`:s<5400?`${Math.round(s/60)} min`:`${(s/3600).toFixed(1)} h`;
  const td=(tr,text,cls)=>{const c=document.createElement('td');c.textContent=text;if(cls)c.className=cls;tr.append(c);return c;};
  function table(head,rows,label){
    const t=document.createElement('table');const cap=document.createElement('caption');cap.className='sr-only';cap.textContent=label;t.append(cap);const h=document.createElement('tr');for(const x of head){const th=document.createElement('th');th.scope='col';th.textContent=x;h.append(th);}
    const th=document.createElement('thead');th.append(h);t.append(th);const b=document.createElement('tbody');for(const r of rows)b.append(r);t.append(b);
    const w=document.createElement('div');w.className='ov-scroll';w.tabIndex=0;w.setAttribute('role','region');w.setAttribute('aria-label',label);w.append(t);return w;
  }
  function draw(list){
    const card=$('queues-card');card.hidden=false;const box=$('queues-body');box.replaceChildren();
    if(!list.length){const p=document.createElement('p');p.className='muted';p.textContent='No queues yet. A queue appears here when several agents share one limited resource, such as a graphics card or a build machine, and a scheduler or script reports it to this app (see Queues in SPEC.md).';box.append(p);return;}
    const now=Date.now()/1000;
    for(const q of list){
      const sec=document.createElement('section');sec.className='q-block';sec.dataset.queue=q.name;
      const h=document.createElement('h4');h.textContent=`${q.name}: ${q.running.length} running, ${q.waiting.length} waiting${q.reserved.length?', reserved':''}`;sec.append(h);
      for(const r of q.reserved){const p=document.createElement('p');p.dataset.kind='reserved';p.textContent=`Reserved${r.who?' by '+r.who:''}${r.until?' until '+new Date(r.until*1000).toLocaleTimeString():''}${r.note?': '+r.note:''}`;sec.append(p);}
      if(q.running.length){const rows=q.running.map(r=>{const tr=document.createElement('tr');tr.dataset.id=r.id;td(tr,r.who||r.id);td(tr,r.slot||'');td(tr,dur(now-r.t));td(tr,r.est_min?`${r.est_min} min`:'');td(tr,r.over?'Over its estimate':'On time',r.over?'q-over':'');return tr;});sec.append(table(['Running','Slot','Running for','Estimate','Status'],rows,q.name+' running'));}
      if(q.waiting.length){const rows=q.waiting.map((r,i)=>{const tr=document.createElement('tr');tr.dataset.id=r.id;td(tr,String(i+1));td(tr,r.who||r.id);td(tr,r.est_min?`${r.est_min} min`:'');td(tr,r.waited_s!=null?dur(r.waited_s):'');td(tr,r.note||'');return tr;});sec.append(table(['#','Waiting','Estimate','Waited','Note'],rows,q.name+' waiting'));}
      box.append(sec);
    }
  }
  async function refresh(){try{draw(await invoke('queues_view'));}catch(e){const box=$('queues-body');$('queues-card').hidden=false;box.replaceChildren();const p=document.createElement('p');p.setAttribute('role','status');p.textContent=window.friendly?window.friendly('Could not read the queues',e):String(e);box.append(p);}}
  $('show-flow').addEventListener('click',()=>{refresh();clearInterval(timer);timer=setInterval(()=>{if(!$('flow-reload').offsetParent){clearInterval(timer);return;}refresh();},5000);});
  $('flow-reload').addEventListener('click',refresh);
})();
