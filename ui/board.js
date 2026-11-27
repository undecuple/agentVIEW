'use strict';
// Project board (read-only view). Agents append lines to kanban/<project>.jsonl in the app's private folder; this page only reads and draws them.
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  const NAMES={queued:'Queued',active:'Active',blocked:'Blocked',review:'In review',done:'Done (24 h)'};
  const STALE_S=3600;let cur='',timer=null,psig='',bsig='',snap=null;
  // A plain Markdown snapshot of the board as it looks now; saving it goes through the project-write dialog (preview, confirm, backup).
  function markdown(b){
    const out=[`# Board: ${b.project}`,'',`Snapshot taken ${new Date().toLocaleString()}. Cards come from agents' updates and are not approvals.`,''];
    for(const col of b.cols){const n=b.cards.filter(x=>x.col===col);out.push(`## ${NAMES[col]} (${n.length})`,'');
      if(!n.length)out.push('_none_');
      for(const k of n)out.push(`- **${(k.title||k.id).replace(/[\r\n]+/g,' ')}** (${k.id}${k.by?', '+k.by:''}, ${new Date(k.t*1000).toISOString().slice(0,16).replace('T',' ')} UTC)${k.note?': '+String(k.note).replace(/[\r\n]+/g,' '):''}`);
      out.push('');}
    return out.join('\n');
  }
  $('board-save-md').onclick=()=>{if(!snap||!snap.cards.length)return;window.ProjWrite.open({content:markdown(snap),name:'BOARD.md',names:['BOARD.md']});};
  async function projects(keep){
    let list=[];try{list=await invoke('kanban_projects');}catch(e){$('board-state').textContent=String(e);}
    const sel=$('board-project');const ns=JSON.stringify(list.map(p=>[p.name,p.cards,p.blocked]));
    if(ns===psig&&!keep)return sel.value;
    if(ns===psig&&[...sel.options].some(o=>o.value===keep))return keep;
    psig=ns;const had=sel.value;sel.replaceChildren();
    for(const p of list){const o=document.createElement('option');o.value=p.name;o.textContent=`${p.name} (${p.cards} cards${p.blocked?`, ${p.blocked} blocked`:''})`;sel.append(o);}
    if(keep&&[...sel.options].some(o=>o.value===keep))sel.value=keep;else if(had&&[...sel.options].some(o=>o.value===had))sel.value=had;
    return sel.value;
  }
  function draw(b){
    const nb=JSON.stringify([b.project,b.cards,Math.floor(Date.now()/300000)]);
    if(nb===bsig)return;bsig=nb;
    const box=$('board-cols');box.replaceChildren();const now=Date.now()/1000;
    for(const col of b.cols){
      const c=document.createElement('section');c.className='board-col';c.dataset.col=col;c.setAttribute('aria-labelledby','bh-'+col);
      const h=document.createElement('h4');h.id='bh-'+col;const n=b.cards.filter(x=>x.col===col);h.textContent=`${NAMES[col]} · ${n.length}`;c.append(h);
      for(const k of n){
        const d=document.createElement('div');d.className='board-card'+(col==='active'&&now-k.t>STALE_S?' stale':'');d.dataset.id=k.id;
        const t=document.createElement('strong');t.textContent=k.title||k.id;d.append(t);
        const s=document.createElement('small');s.className='muted';s.textContent=`${k.id}${k.by?' · '+k.by:''} · ${new Date(k.t*1000).toLocaleString()}${col==='active'&&now-k.t>STALE_S?' · STALE: no update for over an hour':''}`;d.append(s);
        if(k.note){const m=document.createElement('small');m.textContent=k.note;d.append(m);}
        c.append(d);
      }
      box.append(c);
    }
    $('board-instr').value=b.instruction;snap=b;$('board-save-md').disabled=!b.cards.length;
    $('board-state').textContent=b.cards.length?'':'No cards yet. Give the instruction below to an agent working on this project.';
  }
  async function load(name){
    if(!name){cur='';snap=null;$('board-save-md').disabled=true;$('board-cols').replaceChildren();$('board-state').textContent='No project boards yet. Type a project name and press Open, then give the instruction to an agent.';return;}
    try{const b=await invoke('kanban_board',{project:name});cur=name;draw(b);}catch(e){$('board-state').textContent=window.friendly('Could not open the board',e);}
  }
  async function refresh(){const n=await projects(cur);await load(n);}
  $('board-alert').onchange=async()=>{try{$('board-alert').checked=await invoke('kanban_alert',{enabled:$('board-alert').checked});}catch(e){$('board-state').textContent=String(e);}};
  async function loadAlert(){try{$('board-alert').checked=await invoke('kanban_alert',{});}catch{}}
  $('board-project').onchange=()=>load($('board-project').value);
  $('board-open').onclick=async()=>{const n=$('board-new').value.trim();if(!n){await load($('board-project').value);return;}
    try{await invoke('kanban_board',{project:n});}catch(e){$('board-state').textContent=String(e);return;}
    const sel=$('board-project');if(![...sel.options].some(o=>o.value===n)){const o=document.createElement('option');o.value=n;o.textContent=`${n} (0 cards)`;sel.append(o);}
    sel.value=n;$('board-new').value='';await load(n);};
  $('board-copy').onclick=async()=>{const t=$('board-instr');t.focus();t.select();let ok=false;try{await navigator.clipboard.writeText(t.value);ok=true;}catch{try{ok=document.execCommand('copy');}catch{}}$('board-state').textContent=ok?'Copied.':'Select the text and copy it.';};
  AppNav.onShow('flow',()=>{loadAlert();refresh();clearInterval(timer);timer=setInterval(()=>{if(!$('board-card').offsetParent){clearInterval(timer);return;}refresh();},5000);});
  $('flow-reload').addEventListener('click',refresh);
})();
