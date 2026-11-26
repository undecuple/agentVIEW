'use strict';
(() => {
  const $ = id => document.getElementById(id);
  const invoke = command => AppIPC.invoke(command);
  const fields = ['input_tokens','output_tokens','cached_input_tokens','cache_creation_input_tokens','total_tokens'];
  const node = (tag, cls, text) => { const e=document.createElement(tag); if(cls)e.className=cls; if(text!==undefined)e.textContent=text; return e; };
  const providerName = p => p==='codex'?'Codex':'Claude Code';
  const sourceName = value => ({claude_jsonl_v1:'Claude session record (v1)',codex_rollout_v1:'Codex rollout (v1)'})[value]||'No supported source';
  const stateName = value => ({ok:'Read successfully',partial:'Some counters are missing',no_usage:'No replies recorded in its log yet',not_started:'Not started yet',source_not_bound:'No source bound for this session',missing:'Its log file was not found (it may not have had a reply yet); saved values kept',unsupported:'Unsupported source format or path',identity_mismatch:'Source identity mismatch; saved values retained',source_replaced:'Source changed; saved values retained',source_truncated:'Source shortened; saved values retained',counter_reset:'Counter reset; saved values retained',too_large:'Source limit reached',read_error:'Source unreadable; saved values retained',cancelled:'Check cancelled',budget_exceeded:'Refresh limit reached'})[value]||'Source unavailable';
  const count = n => Number.isSafeInteger(n)&&n>=0 ? n.toLocaleString() : 'Unavailable';
  // Claude Code can log less output than it used, so its output and total are shown as "at least" (≥).
  const lowerFields = ['output_tokens','total_tokens'];
  const shown = (field, n, lower) => (lower && lowerFields.includes(field) && Number.isSafeInteger(n) && n>=0) ? `≥ ${count(n)}` : count(n);
  const stamp = value => typeof value==='string' ? value : 'Unavailable';
  let snapshot={sessions:[]},busy=false,version=0;
  function inPeriod(date) {
    const period=$('usage-period').value;
    if(period==='all')return true;
    const today=new Date();today.setUTCHours(0,0,0,0);
    const days=period==='today'?1:Number(period);
    return date>=new Date(today.getTime()-(days-1)*86400000).toISOString().slice(0,10)&&date<=today.toISOString().slice(0,10);
  }
  // A subtotal is available only if at least one source reports that field.
  // Never substitute zero for an absent measurement or an unobserved day.
  function sum(items) {
    const result={};
    for(const field of fields){
      const values=items.map(t=>t?.[field]).filter(n=>Number.isSafeInteger(n)&&n>=0);
      const total=values.reduce((a,b)=>a+b,0);
      result[field]=values.length&&Number.isSafeInteger(total)?total:null;
    }
    return result;
  }
  function totals(session) {
    return $('usage-period').value==='all'?session.totals:sum(session.daily.filter(d=>inPeriod(d.date)).map(d=>d.totals));
  }
  function cells(row, values, samples, denominator, lower){
    for(const field of fields){
      const cell=node('td','');cell.dataset.value=String(values?.[field]??'unavailable');
      cell.append(node('span','usage-number',shown(field,values?.[field],lower)));
      if(samples&&values?.[field]!=null)cell.append(node('small','',`${samples.filter(t=>t?.[field]!=null).length}/${denominator} sessions`));
      row.append(cell);
    }
  }
  function render() {
    const sessions=snapshot.sessions.filter(s=>!$('usage-provider').value||s.provider===$('usage-provider').value);
    $('usage-providers').replaceChildren();
    for(const provider of ['claude','codex'].filter(p=>!$('usage-provider').value||p===$('usage-provider').value)){
      const rows=sessions.filter(s=>s.provider===provider),values=sum(rows.map(totals));
      const measured=rows.filter(s=>totals(s)?.total_tokens!=null).length;
      const card=node('div','usage-card');card.dataset.provider=provider;
      card.append(node('h3','',providerName(provider)),node('strong','',shown('total_tokens',values.total_tokens,provider==='claude')),node('p','',provider==='claude'?'At least this many tokens (Claude Code may log less output than it used)':'Reported token subtotal'),node('small','',`${measured} of ${rows.length} sessions report a total in this period`));
      const breakdown=node('dl','usage-breakdown');
      for(const [label,field] of [['Input','input_tokens'],['Output','output_tokens'],['Cache read','cached_input_tokens'],['Cache write','cache_creation_input_tokens']]){const value=node('dd','',shown(field,values[field],provider==='claude'));if(values[field]!=null)value.append(node('small','',` · ${rows.filter(s=>totals(s)?.[field]!=null).length}/${rows.length} sessions`));breakdown.append(node('dt','',label),value);};
      card.append(breakdown);
      $('usage-providers').append(card);
    }
    $('usage-agents').replaceChildren();
    for(const s of sessions){
      const row=node('tr','');row.dataset.session=s.app_session_id;
      const label=node('td','');label.append(node('strong','',s.label),node('small','',`${providerName(s.provider)} · Coordinator`),node('small','',`App ${s.app_session_id}`),node('small','',`Source: ${sourceName(s.source)} · ${stateName(s.status)}`));
      label.append(node('small','',`Last checked: ${stamp(s.last_checked_at)}`),node('small','',`Last measured: ${stamp(s.last_measured_at)}`));
      row.append(label);cells(row,totals(s),null,0,s.output_lower_bound);$('usage-agents').append(row);
    }
    if(!sessions.length){const row=node('tr','');const cell=node('td','muted','No registered live sessions in this view. Usage is Unavailable.');cell.colSpan=6;row.append(cell);$('usage-agents').append(row);}
    const days=new Map();
    for(const s of sessions)for(const day of s.daily){if(inPeriod(day.date)){if(!days.has(day.date))days.set(day.date,[]);days.get(day.date).push(day.totals);}}
    $('usage-days').replaceChildren();
    for(const date of [...days.keys()].sort().reverse()){
      const row=node('tr',''),label=node('td','');
      const measured=days.get(date).filter(t=>t.total_tokens!=null).length;
      label.append(node('strong','',date),node('small','',`${measured} of ${sessions.length} sessions report a total`));
      row.append(label);cells(row,sum(days.get(date)),days.get(date),sessions.length,sessions.some(s=>s.output_lower_bound));$('usage-days').append(row);
    }
    if(!days.size){const row=node('tr','');const cell=node('td','muted','No reported daily measurements in this period.');cell.colSpan=6;row.append(cell);$('usage-days').append(row);}
  }
  async function load(refresh) {
    if(busy)return;busy=true;const request=++version;$('usage-refresh').disabled=true;
    $('usage-notice').textContent=refresh?'Reading matching local session records…':'Loading saved measurements…';
    try{
      const next=await invoke(refresh?'usage_refresh':'usage_snapshot');
      if(request!==version)return;
      snapshot=next;render();
      $('usage-notice').textContent=refresh?'Local check complete. See each source status and measurement time.':'Showing saved measurements. Refresh to check local sources.';
    }catch(e){$('usage-notice').textContent=`Usage could not be ${refresh?'refreshed':'loaded'}: ${String(e)}. Displayed values have not been refreshed.`;render();}
    finally{busy=false;$('usage-refresh').disabled=false;}
  }
  let timer=null,lastDefaults=null;
  const active=()=>!$('usage').hidden&&!document.hidden&&document.hasFocus();
  function schedule(){
    clearTimeout(timer);timer=null;
    const seconds=AppPrefs.current?.usage_refresh_seconds||0;
    if(!seconds||!active())return;
    timer=setTimeout(async()=>{timer=null;if(active())await load(true);schedule();},seconds*1000);
  }
  window.addEventListener('agentview:preferences',e=>{
    const p=e.detail,key=`${p.usage_provider}:${p.usage_period}`;
    if(lastDefaults!==key){lastDefaults=key;$('usage-provider').value=p.usage_provider;$('usage-period').value=p.usage_period;render();}schedule();
  });
  window.addEventListener('agentview:navigate',e=>{if(e.detail==='usage'){render();load(false);}schedule();});
  document.addEventListener('visibilitychange',schedule);window.addEventListener('focus',schedule);window.addEventListener('blur',()=>{clearTimeout(timer);timer=null;});
  $('usage-refresh').onclick=()=>load(true);
  $('usage-provider').onchange=render;$('usage-period').onchange=render;
})();
