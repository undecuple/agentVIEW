'use strict';
// Report-an-issue helper (Settings, About). Builds text to paste into this project's GitHub issue forms. It contacts nothing: no token, no request.
// The link only prefills GitHub's own form in the person's browser; the person reviews and submits it there.
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  const REPO='https://github.com/undecuple/agentVIEW',LINK_MAX=6000;
  const TOKEN=/(ghp_|github_pat_|sk-[A-Za-z0-9]|AKIA[0-9A-Z]{8}|xox[abprs]-|-----BEGIN [A-Z ]*PRIVATE KEY|Bearer\s+[A-Za-z0-9._-]{12,})/;
  const KINDS={bug:{template:'bug_report.yml',label:'What happened, and what did you expect?',title:'Bug: '},feature:{template:'feature_request.yml',label:'What would you like, and why?',title:'Idea: '}};
  let version='';
  const scrub=t=>t.replace(/\/home\/[^\s/"')]+/g,'~').replace(/\/Users\/[^\s/"')]+/g,'~');
  function build(){
    const k=KINDS[$('ih-kind').value],desc=scrub($('ih-what').value.trim()),title=($('ih-title').value.trim()||'').slice(0,120);
    const sys=scrub($('ih-system').value.trim()),bug=$('ih-kind').value==='bug';
    $('ih-what-label').textContent=k.label;$('ih-fields').hidden=!bug;
    const bad=TOKEN.test(desc+' '+title+' '+sys);
    const q=new URLSearchParams({template:k.template});if(title)q.set('title',title);if(desc)q.set('what',desc);
    if(bug){q.set('build',version);if(sys)q.set('system',sys);}
    const link=`${REPO}/issues/new?${q.toString()}`;
    const tooLong=link.length>LINK_MAX,missing=!desc||(bug&&!sys);
    $('ih-link').value=tooLong?'':link;
    for(const id of ['ih-copy-link','ih-copy-what','ih-copy-version','ih-copy-system','ih-copy-title'])$(id).disabled=bad||(id==='ih-copy-link'&&(tooLong||missing))||(id==='ih-copy-what'&&!desc);
    $('ih-note').textContent=bad?'This text looks like it contains a key or token. Remove it before copying; the buttons stay off until you do.'
      :missing?(bug?'Fill in what happened and your Linux distribution and WebKitGTK version (the form needs them).':'Say what you would like and why.')
      :tooLong?'The text is too long for a link. Use the copy buttons and paste each part into the form.':'Ready. Nothing has been sent. Open the link in your browser, check the form, then submit it there.';
    return {desc,title,sys,bug};
  }
  async function copy(text,msg){try{await navigator.clipboard.writeText(text);}catch{const t=document.createElement('textarea');t.value=text;document.body.append(t);t.select();try{document.execCommand('copy');}catch{}t.remove();}$('ih-note').textContent=msg;}
  async function init(){
    try{const a=await invoke('app_about');version=`agentVIEW ${a.app_version} (build ${a.build})`;}catch{version='agentVIEW (version unknown)';}
    $('ih-version').value=version;
    const wk=(navigator.userAgent.match(/AppleWebKit\/[\d.]+/)||[''])[0];
    if(!$('ih-system').value)$('ih-system').placeholder=`Linux distribution and WebKitGTK version${wk?` (this window reports ${wk})`:''}`;
    build();
  }
  for(const id of ['ih-kind','ih-title','ih-what','ih-system'])$(id).addEventListener('input',build);
  $('ih-copy-link').onclick=()=>{build();copy($('ih-link').value,'Link copied. Paste it into your browser.');};
  $('ih-copy-title').onclick=()=>copy($('ih-title').value.trim(),'Title copied.');
  $('ih-copy-what').onclick=()=>copy(build().desc,'Description copied (home-folder paths were shortened to ~).');
  $('ih-copy-version').onclick=()=>copy(version,'Version copied.');
  $('ih-copy-system').onclick=()=>copy(build().sys,'System text copied.');
  $('show-settings').addEventListener('click',init);
})();
