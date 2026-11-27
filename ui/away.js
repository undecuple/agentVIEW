'use strict';
// "Stepping away" button (Overview). Away mode is a flag file plus an instruction agents can read; the app lists the questions agents park while it is on.
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  let timer=null,away=false,sig='';
  function draw(s){
    away=s.away;const b=$('away-toggle');b.textContent=s.away?"I'm back":"I'm stepping away";
    $('away-state').textContent=s.away?`Away since ${new Date(s.since*1000).toLocaleTimeString()}. ${s.pending.length} parked question${s.pending.length===1?'':'s'}. Agents only skip decision boxes if they follow the away instruction (see below).`:(s.pending.length?`You're back. ${s.pending.length} question${s.pending.length===1?' was':'s were'} parked while you were away.`:'Off. Agents only stop asking you when they follow the away instruction (see below).');
    $('away-hook').value=s.hook_snippet;$('away-instr').value=s.instruction;
    const nsig=JSON.stringify([s.pending.map(d=>[d.id,d.t,d.question])]);if(nsig===sig)return;sig=nsig;
    const box=$('away-list');box.replaceChildren();box.setAttribute('aria-label','Parked questions');
    for(const d of s.pending){
      const p=document.createElement('li');p.className='board-card';p.dataset.id=d.id;
      const q=document.createElement('strong');q.textContent=d.question;p.append(q);
      const m=document.createElement('small');m.className='muted';m.textContent=`${d.from||'an agent'}${d.project?' · '+d.project:''} · ${new Date(d.t*1000).toLocaleString()}${d.options.length?' · options: '+d.options.join(' / '):''}`;p.append(m);
      box.append(p);
    }
    $('away-seen').hidden=!s.pending.length;
  }
  async function load(a={}){try{draw(await invoke('away_status',a));}catch(e){$('away-state').textContent=window.friendly('Could not update away mode',e);}}
  $('away-toggle').onclick=()=>load({away:!away});
  $('away-seen').onclick=()=>load({ack:true});
  AppNav.onShow('overview',()=>{load();clearInterval(timer);timer=setInterval(()=>{if(!$('away-card').offsetParent){clearInterval(timer);return;}load();},5000);});
  try{if(localStorage.getItem('av-new-note')!=='1')$('new-note').hidden=false;}catch{$('new-note').hidden=false;}
  $('new-note-close').onclick=()=>{$('new-note').hidden=true;try{localStorage.setItem('av-new-note','1');}catch{}};
  load();
})();
