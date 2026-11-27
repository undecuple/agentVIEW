#!/usr/bin/env python3
"""Synthetic provider CLIs for GUI controls. No accounts, network or inference."""
import fcntl
import json
import os
from pathlib import Path
import struct
import sys
import termios
import time
import uuid
import datetime
import re

provider='codex' if 'codex' in Path(sys.argv[0]).name else 'claude'
args=sys.argv[1:]
if args in (['--help'],['resume','--help']):
    print('app-server resume --no-daemon' if provider=='codex' else '--session-id --resume --name --remote-control')
    raise SystemExit(0)
if args==['--version']:
    print(f'fixture-1 (synthetic {provider})')
    raise SystemExit(0)
state=Path('.agentview-fixture.json')
fixture_root=Path(os.environ['AGENT_APP_TEST_PROJECT_ROOT'])
def usage_path(sid):
    if provider=='codex':
        return fixture_root/'codex-home/sessions/2026/11/22'/f'rollout-{sid}.jsonl'
    slug=re.sub(r'[^a-zA-Z0-9]','-',str(Path.cwd()))
    return fixture_root/'claude-home/projects'/slug/f'{sid}.jsonl'

def write_usage(sid, measured=False):
    path=usage_path(sid);path.parent.mkdir(parents=True,exist_ok=True,mode=0o700)
    today=datetime.datetime.now(datetime.timezone.utc).date()
    stamps=[str(today-datetime.timedelta(days=1))+'T12:00:00Z',str(today)+'T12:00:00Z']
    if provider=='codex':
        rows=[{'type':'session_meta','payload':{'id':sid,'cwd':str(Path.cwd())}}]
        if measured:
            for stamp,counts in [(stamps[0],(100,40,20,120)),(stamps[0],(100,40,20,120)),(stamps[1],(160,60,35,195))]:
                rows.append({'timestamp':stamp,'type':'event_msg','payload':{'type':'token_count','info':{'total_token_usage':dict(zip(['input_tokens','cached_input_tokens','output_tokens','total_tokens'],counts))}}})
    else:
        rows=[]
        if measured:
            for stamp,mid,counts in [(stamps[0],'fixture-message-1',(10,20,3)),(stamps[0],'fixture-message-1',(12,25,4)),(stamps[1],'fixture-message-2',(20,30,5))]:
                rows.append({'timestamp':stamp,'type':'assistant','sessionId':sid,'cwd':str(Path.cwd()),'message':{'id':mid,'role':'assistant','usage':dict(zip(['input_tokens','cache_read_input_tokens','cache_creation_input_tokens'],counts)),'content':[{'type':'text','text':'SYNTHETIC_CONTENT_MUST_NOT_PERSIST'}]}})
    with path.open('w') as output:
        for row in rows:output.write(json.dumps(row)+'\n')
    path.chmod(0o600)

if provider=='codex' and args[:1]==['app-server']:
    pending=None
    def reply(request,result):
        print(json.dumps({'id':request['id'],'result':result}),flush=True)
    def thread(saved):
        return {'id':saved['id'],'cwd':str(Path.cwd()),'ephemeral':False,'path':str(usage_path(saved['id']))}
    for line in sys.stdin:
        request=json.loads(line)
        method=request['method']
        if method=='initialized':
            continue
        if method=='initialize':
            reply(request,{})
        elif method=='thread/start':
            if Path('.hang-bootstrap').exists():
                Path('.helper-started').write_text(str(os.getpid()))
                time.sleep(60)
            assert not state.exists(), 'must not re-bootstrap a saved thread'
            count=Path('.bootstrap-count')
            count.write_text(str(int(count.read_text())+1 if count.exists() else 1))
            pending={'id':str(uuid.uuid4()),'provider':'codex','turns':[]}
            reply(request,{'thread':thread(pending)})
        elif method=='thread/name/set':
            assert pending and request['params']['threadId']==pending['id']
            state.write_text(json.dumps(pending))
            write_usage(pending['id'])
            reply(request,{})
        elif method=='thread/read':
            saved=json.loads(state.read_text())
            assert request['params']['threadId']==saved['id']
            reply(request,{'thread':thread(saved)})
        else:
            print(json.dumps({'id':request.get('id'),'error':{'message':'fixture refuses unknown action'}}),flush=True)
    raise SystemExit(0)

Path('.fixture-pid').write_text(str(os.getpid()))
assert all(os.isatty(fd) for fd in (0,1,2)), 'All streams must be PTYs'
if provider=='codex':
    assert len(args)==3 and args[0]=='resume' and args[2]=='--no-daemon',args
    mode='resume'
    sid=args[1]
else:
    # Optional name and Remote Control flags (as the real CLI accepts them); recorded for the test to read back.
    extra=[];rest=list(args)
    while len(rest)>2:
        if rest[-2]=='--name':extra.append(('name',rest[-1]));rest=rest[:-2]
        elif rest[-2]=='--remote-control':extra.append(('remote',rest[-1]));rest=rest[:-2]
        else:break
    Path('.agentview-fixture-flags.json').write_text(json.dumps(extra))
    args=rest
    assert len(args)==2 and args[0] in ('--session-id','--resume'),args
    mode=args[0]
    sid=args[1]
assert len(sid)==36
if mode in ('resume','--resume'):
    if not state.exists():
        print('MISSING PROVIDER TRANSCRIPT',flush=True)
        raise SystemExit(3)
    saved=json.loads(state.read_text())
    assert saved['id']==sid and saved.get('provider','claude')==provider
else:
    assert not state.exists()
    saved={'id':sid,'provider':provider,'turns':[]}
    state.write_text(json.dumps(saved))
print(f'\033[38;5;211mSYNTHETIC {provider.upper()} CLI {sid}\033[0m',flush=True)
print(f'PROJECT {Path.cwd().name} MODE {mode} RESTORED {len(saved["turns"])}',flush=True)
print('<script>window.terminalPwned=1</script>\033]52;c;Tk9UX0FfU0VDUkVU\007',flush=True)
print('FIXTURE READY',flush=True)
for line in sys.stdin:
    line=line.rstrip('\r\n')
    if line in ('/fixture-usage-hide','/fixture-usage-restore'):
        path=usage_path(sid);hidden=path.with_suffix('.hidden')
        if line.endswith('-hide'):path.rename(hidden)
        else:hidden.rename(path)
        print('USAGE '+('HIDDEN' if line.endswith('-hide') else 'RESTORED'),flush=True)
        continue
    if line=='/fixture-resume-other' and provider=='claude':
        # Like /resume inside the real CLI: the process now writes a different top-level transcript in the same folder.
        sid=str(uuid.uuid4());saved['id']=sid;state.write_text(json.dumps(saved));write_usage(sid,measured=True)
        print('RESUMED OTHER '+sid,flush=True)
        continue
    if line.startswith('/size '):
        rows,cols,_,_=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,b'\0'*8))
        print(f'SIZE {line[6:]} {cols} {rows}',flush=True)
        continue
    if line=='/quit':
        break
    saved['turns'].append(line)
    state.write_text(json.dumps(saved))
    write_usage(sid,measured=True)
    print('ACK '+line,flush=True)
