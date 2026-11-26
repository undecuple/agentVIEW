//! Read-only, exact app-managed transcript accounting. Provider JSONL is an
//! internal/versioned format: absent or unsupported fields remain unknown.
//! Content is parsed transiently, never copied into app storage or snapshots.
// Source map: Claude docs/en/sessions#where-transcripts-are-stored describes the
// exact path and warns JSONL is internal; agent-sdk/cost-tracking establishes
// message-ID dedupe and accurate input/cache accounting. It does not establish
// finality of raw on-disk output counts, so those remain unknown here.
// Codex's installed ThreadTokenUsageUpdatedNotification schema and upstream
// codex-rs/protocol/src/protocol.rs TokenUsageInfo define cumulative totals and
// cache/reasoning subsets. Only exact saved thread.path files are read, never
// live notifications, terminal text, latest-thread discovery or account quotas.
use super::*;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

// Even a provider rollup of all 64 app sessions remains exactly JS-safe.
const SAFE: u64 = 1_000_000_000_000;
const FILE_BYTES: u64 = 32 * 1024 * 1024;
const REFRESH_BYTES: u64 = 64 * 1024 * 1024;
const LINE_BYTES: usize = 1024 * 1024;
const RECORDS: usize = 100_000;

#[derive(Clone, Debug, Default)]
pub struct UsageRoots { pub claude: Option<PathBuf>, pub codex: Option<PathBuf> }

/// Input is normalized to include cache reads/writes. Cache fields are subsets,
/// not extra amounts to add to input/total. None means no supported measurement.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageTotals {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageDay { pub date: String, pub totals: UsageTotals }
#[derive(Clone, Debug, Serialize)]
pub struct UsageSession {
    pub app_session_id: String,
    pub provider: Provider,
    pub native_session_id: Option<String>,
    pub label: String,
    pub source: String,
    pub status: String,
    pub last_checked_at: Option<String>,
    pub last_measured_at: Option<String>,
    pub totals: UsageTotals,
    pub daily: Vec<UsageDay>,
    /// True for Claude Code: its log is the only source, and it can record less output than was used, so output and total are "at least".
    pub output_lower_bound: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct UsageSnapshot { pub sessions: Vec<UsageSession> }

fn now() -> String { DateTime::<Utc>::from(std::time::SystemTime::now()).to_rfc3339_opts(SecondsFormat::Secs,true) }
fn timestamp(value: &Value) -> Result<(String,String)> {
    let value=value.as_str().ok_or("unsupported")?;
    let date=DateTime::parse_from_rfc3339(value).map_err(|_|"unsupported")?.with_timezone(&Utc);
    Ok((date.format("%Y-%m-%d").to_string(),date.to_rfc3339_opts(SecondsFormat::Millis,true)))
}
fn number(value: &Value,key:&str) -> Result<Option<u64>> {
    match value.get(key) {
        None|Some(Value::Null)=>Ok(None),
        Some(v)=>v.as_u64().filter(|n|*n<=SAFE).map(Some).ok_or_else(||"unsupported".into()),
    }
}
fn add(a:Option<u64>,b:Option<u64>) -> Result<Option<u64>> {
    match(a,b){(Some(a),Some(b))=>a.checked_add(b).filter(|n|*n<=SAFE).map(Some).ok_or_else(||"unsupported".into()),_=>Ok(None)}
}
impl UsageTotals {
    fn values(&self)->[Option<u64>;5]{[self.input_tokens,self.output_tokens,self.cached_input_tokens,self.cache_creation_input_tokens,self.total_tokens]}
    fn from(v:[Option<u64>;5])->Self{Self{input_tokens:v[0],output_tokens:v[1],cached_input_tokens:v[2],cache_creation_input_tokens:v[3],total_tokens:v[4]}}
    fn sum(&self,other:&Self)->Result<Self>{let a=self.values();let b=other.values();let mut out=[None;5];for i in 0..5{out[i]=add(a[i],b[i])?;}Ok(Self::from(out))}
    fn dominates(&self,other:&Self)->bool{self.values().iter().zip(other.values()).all(|(a,b)|match(a,b){(_,None)=>true,(Some(a),Some(b))=>*a>=b,(None,Some(_))=>false})}
    fn delta(&self,prior:&Self)->Result<Self>{let a=self.values();let b=prior.values();let mut out=[None;5];for i in 0..5{out[i]=match(a[i],b[i]){(Some(a),Some(b))=>Some(a.checked_sub(b).ok_or("counter_reset")?),(None,_)=>None,(Some(_),None)=>None};}Ok(Self::from(out))}
    fn decreased_from(&self,prior:&Self)->bool{self.values().iter().zip(prior.values()).any(|(a,b)|matches!((a,b),(Some(a),Some(b)) if *a<b))}
    fn any(&self)->bool{self.values().iter().any(Option::is_some)}
}
pub(super) fn initialize(tx:&rusqlite::Transaction<'_>)->Result<()> {
    sql(tx.execute_batch("CREATE TABLE IF NOT EXISTS usage_sources(app_session_id TEXT PRIMARY KEY,path TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS usage_readings(app_session_id TEXT PRIMARY KEY,source TEXT NOT NULL,status TEXT NOT NULL,
        checked TEXT,measured TEXT,totals TEXT NOT NULL,daily TEXT NOT NULL,device INTEGER,inode INTEGER,bytes INTEGER);
        PRAGMA user_version=3;"))
}
pub(super) fn bind_codex(tx:&rusqlite::Transaction<'_>,id:&str,path:&str)->Result<()> {
    sql(tx.execute("INSERT INTO usage_sources VALUES (?1,?2) ON CONFLICT(app_session_id) DO UPDATE SET path=excluded.path",params![id,path]))?;Ok(())
}
impl SessionManager {
    /// Stored numeric data only. This never reads provider directories or runs a CLI.
    pub fn usage_snapshot(&mut self)->Result<UsageSnapshot>{
        let roster=self.list()?;let mut sessions=Vec::new();
        for session in roster {
            let mut item=UsageSession{app_session_id:session.id.clone(),provider:session.provider,native_session_id:session.provider_session_id,
                label:session.label,source:source(session.provider).into(),status:"unavailable".into(),last_checked_at:None,last_measured_at:None,
                totals:UsageTotals::default(),daily:Vec::new(),output_lower_bound:session.provider==Provider::Claude};
            let saved:Option<(String,String,Option<String>,Option<String>,String,String)>=sql(self.connection.query_row(
                "SELECT source,status,checked,measured,totals,daily FROM usage_readings WHERE app_session_id=?1",[&session.id],
                |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional())?;
            if let Some((source,status,checked,measured,totals,daily))=saved {
                item.source=source;item.status=status;item.last_checked_at=checked;item.last_measured_at=measured;
                item.totals=serde_json::from_str(&totals).map_err(|_|"Stored usage totals are invalid")?;
                item.daily=serde_json::from_str(&daily).map_err(|_|"Stored daily usage is invalid")?;
            }
            sessions.push(item);
        }Ok(UsageSnapshot{sessions})
    }
    /// Explicit refresh: exact app-owned identities only, bounded bytes/records,
    /// cancellation checked between chunks. No provider process or prompt replay.
    pub fn refresh_usage(&mut self,roots:&UsageRoots)->Result<UsageSnapshot>{
        let roster=self.list()?;let mut budget=REFRESH_BYTES;let checked=now();
        for session in roster {
            let prior:Option<(Option<u64>,Option<u64>,Option<u64>,String)>=sql(self.connection.query_row(
                "SELECT device,inode,bytes,totals FROM usage_readings WHERE app_session_id=?1",[&session.id],
                |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional())?;
            let result:Result<(Parsed,fs::Metadata)>=(||{
                if self.cancel.load(Ordering::Acquire){return Err("cancelled".into());}
                if budget==0{return Err("budget_exceeded".into());}
                let (root,relative)=self.usage_location(&session,roots)?;
                let mut file=sys::usage_file(&root,&relative).map_err(|e|if e.kind()==std::io::ErrorKind::NotFound{"missing"}else{"read_error"})?;
                let meta=file.metadata().map_err(|_|"read_error")?;
                if !meta.is_file() || meta.uid()!=sys::uid() || meta.mode()&0o022!=0 || meta.nlink()!=1{return Err("identity_mismatch".into());}
                if let Some((Some(dev),Some(ino),Some(bytes),_))=&prior {
                    if(*dev,*ino)!=(meta.dev(),meta.ino()){return Err("source_replaced".into());}
                    if meta.len()<*bytes{return Err("source_truncated".into());}
                }
                if meta.len()>FILE_BYTES{return Err("too_large".into());}
                if meta.len()>budget{budget=0;return Err("budget_exceeded".into());}
                let parsed=parse_file(&mut file,meta.len(),&session,&self.cancel,&mut budget)?;
                let after=file.metadata().map_err(|_|"read_error")?;
                let current=fs::symlink_metadata(root.join(relative)).map_err(|_|"source_replaced")?;
                if after.len()<meta.len() || current.ino()!=meta.ino() || current.dev()!=meta.dev() || !current.is_file(){return Err("source_replaced".into());}
                if after.nlink()!=1 || after.uid()!=sys::uid() || after.mode()&0o022!=0{return Err("identity_mismatch".into());}
                if let Some((_,_,_,old))=&prior {
                    let old:UsageTotals=serde_json::from_str(old).map_err(|_|"unsupported")?;
                    if parsed.totals.decreased_from(&old){return Err("counter_reset".into());}
                }
                Ok((parsed,meta))
            })();
            match result {
                Ok((parsed,meta)) if parsed.totals.any()=>{
                    let totals=serde_json::to_string(&parsed.totals).map_err(|_|"Cannot encode numeric usage")?;
                    let daily=serde_json::to_string(&parsed.daily).map_err(|_|"Cannot encode numeric usage")?;
                    sql(self.connection.execute("INSERT INTO usage_readings VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                        ON CONFLICT(app_session_id) DO UPDATE SET source=excluded.source,status=excluded.status,checked=excluded.checked,
                        measured=excluded.measured,totals=excluded.totals,daily=excluded.daily,device=excluded.device,inode=excluded.inode,bytes=excluded.bytes",
                        params![session.id,source(session.provider),if parsed.partial{"partial"}else{"ok"},checked,parsed.measured,totals,daily,meta.dev(),meta.ino(),meta.len()]))?;
                },
                other=>{
                    let status=match other{Ok(_)=>"no_usage".to_string(),Err(status)=>status};
                    sql(self.connection.execute("INSERT INTO usage_readings(app_session_id,source,status,checked,totals,daily) VALUES (?1,?2,?3,?4,'{}','[]')
                        ON CONFLICT(app_session_id) DO UPDATE SET status=excluded.status,checked=excluded.checked",
                        params![session.id,source(session.provider),status,checked]))?;
                }
            }
        }self.usage_snapshot()
    }
    pub(crate) fn usage_location(&self,session:&SessionInfo,roots:&UsageRoots)->Result<(PathBuf,PathBuf)>{
        if !session.attempted{return Err("not_started".into());}
        let native=session.provider_session_id.as_deref().ok_or("not_started")?;
        codex::validate_uuid(native).map_err(|_|"identity_mismatch")?;
        let root=match session.provider{Provider::Claude=>&roots.claude,Provider::Codex=>&roots.codex}.as_ref().ok_or("missing")?;
        if !root.is_absolute() || fs::canonicalize(root).map_err(|_|"missing")?!=*root{return Err("unsupported".into());}
        let relative=match session.provider{
            Provider::Claude=>{
                if std::env::var_os("CLAUDE_CODE_PROJECT_DIR_NAME").is_some(){return Err("unsupported".into());}
                let slug:String=session.project_dir.chars().map(|c|if c.is_ascii_alphanumeric(){c}else{'-'}).collect();
                if slug.len()>200{return Err("unsupported".into());}
                PathBuf::from("projects").join(slug).join(format!("{native}.jsonl"))
            },
            Provider::Codex=>{
                let path:Option<String>=sql(self.connection.query_row("SELECT path FROM usage_sources WHERE app_session_id=?1",[&session.id],|r|r.get(0)).optional())?;
                let path=PathBuf::from(path.ok_or("source_not_bound")?);
                let relative=path.strip_prefix(root).map_err(|_|"identity_mismatch")?.to_path_buf();
                let parts:Vec<_>=relative.components().collect();
                if parts.len()!=5 || parts.iter().any(|c|!matches!(c,std::path::Component::Normal(_)))
                    || !relative.starts_with("sessions") || !path.file_name().and_then(|n|n.to_str()).is_some_and(|n|n.starts_with("rollout-")&&n.ends_with(&format!("-{native}.jsonl"))) {
                    return Err("identity_mismatch".into());
                }
                let date=format!("{}-{}-{}",parts[1].as_os_str().to_string_lossy(),parts[2].as_os_str().to_string_lossy(),parts[3].as_os_str().to_string_lossy());
                chrono::NaiveDate::parse_from_str(&date,"%Y-%m-%d").map_err(|_|"identity_mismatch")?;
                relative
            }
        };Ok((root.clone(),relative))
    }
}
fn source(provider:Provider)->&'static str{match provider{Provider::Claude=>"claude_jsonl_v1",Provider::Codex=>"codex_rollout_v1"}}

struct Parsed {totals:UsageTotals,daily:Vec<UsageDay>,measured:Option<String>,partial:bool}
fn parse_file(file:&mut File,length:u64,session:&SessionInfo,cancel:&AtomicBool,budget:&mut u64)->Result<Parsed>{
    let mut parser=Parser::new(session);let mut pending=Vec::new();let mut remaining=length;let mut records=0;
    while remaining>0 {
        if cancel.load(Ordering::Acquire){return Err("cancelled".into());}
        let mut bytes=[0u8;8192];let cap=(remaining as usize).min(bytes.len());
        let n=file.read(&mut bytes[..cap]).map_err(|_|"read_error")?;
        if n==0{return Err("source_truncated".into());}
        remaining-=n as u64;*budget=budget.saturating_sub(n as u64);
        for byte in &bytes[..n] {
            if *byte==b'\n' {
                records+=1;if records>RECORDS{return Err("too_large".into());}
                if !pending.is_empty(){let value:Value=serde_json::from_slice(&pending).map_err(|_|"unsupported")?;parser.record(&value)?;}
                pending.clear();
            }else{if pending.len()>=LINE_BYTES{return Err("too_large".into());}pending.push(*byte);}
        }
    }
    parser.finish(!pending.is_empty())
}
struct Parser<'a>{session:&'a SessionInfo,messages:BTreeMap<String,(String,String,UsageTotals)>,outputs:BTreeMap<String,u64>,
    previous:Option<UsageTotals>,seen:HashSet<(String,[Option<u64>;5])>,daily:BTreeMap<String,UsageTotals>,measured:Option<String>,identity:bool}
impl<'a> Parser<'a>{
    fn new(session:&'a SessionInfo)->Self{Self{session,messages:BTreeMap::new(),outputs:BTreeMap::new(),previous:None,seen:HashSet::new(),daily:BTreeMap::new(),measured:None,identity:false}}
    fn record(&mut self,v:&Value)->Result<()>{
        let native=self.session.provider_session_id.as_deref().ok_or("not_started")?;
        match self.session.provider{
            Provider::Claude=>{
                if v.get("type").and_then(Value::as_str)!=Some("assistant") || v.get("isSidechain").and_then(Value::as_bool)==Some(true) || v.get("parent_tool_use_id").is_some_and(|p|!p.is_null()){return Ok(());}
                let Some(usage)=v.pointer("/message/usage").filter(|u|u.is_object())else{return Ok(());};
                if v.get("sessionId").and_then(Value::as_str)!=Some(native)||v.get("cwd").and_then(Value::as_str)!=Some(&self.session.project_dir){return Err("identity_mismatch".into());}
                self.identity=true;
                let id=v.pointer("/message/id").and_then(Value::as_str).filter(|id|!id.is_empty()&&id.len()<=200).ok_or("unsupported")?;
                let(day,time)=timestamp(v.get("timestamp").ok_or("unsupported")?)?;
                let raw=number(usage,"input_tokens")?;let read=number(usage,"cache_read_input_tokens")?;let creation=number(usage,"cache_creation_input_tokens")?;
                let totals=UsageTotals{input_tokens:add(add(raw,read)?,creation)?,cached_input_tokens:read,cache_creation_input_tokens:creation,..Default::default()};
                // Unsupported-only/empty usage is not a numeric observation:
                // it must not reserve a message day or poison later totals.
                if !totals.any(){return Ok(());}
                // Output is kept apart (the largest value seen for a message), because Claude Code can log a smaller figure than it used.
                // It is therefore shown as "at least", never as exact, and it never takes part in the coherent-snapshot rule below.
                if let Some(o)=number(usage,"output_tokens")?{let e=self.outputs.entry(id.into()).or_insert(0);if o>*e{*e=o;}}
                // On-disk per-step output finality is not established by the SDK
                // contract; don't convert its possible placeholder into usage.
                if let Some((_,old_time,old))=self.messages.get_mut(id){
                    // Retain one coherent snapshot. Independently maximizing
                    // cache fields can fabricate a usage tuple that never existed.
                    if totals.dominates(old){
                        // Timestamp belongs to the chosen coherent tuple.
                        // Equal/dominated replay is not a new measurement.
                        if totals!=*old{*old=totals;*old_time=time;}
                    }
                    else if !old.dominates(&totals){return Err("unsupported".into());}
                }else{self.messages.insert(id.into(),(day,time,totals));}
            },
            Provider::Codex=>{
                if v.get("type").and_then(Value::as_str)==Some("session_meta"){
                    if v.pointer("/payload/id").and_then(Value::as_str)!=Some(native)||v.pointer("/payload/cwd").and_then(Value::as_str)!=Some(&self.session.project_dir){return Err("identity_mismatch".into());}
                    self.identity=true;return Ok(());
                }
                if v.get("type").and_then(Value::as_str)!=Some("event_msg")||v.pointer("/payload/type").and_then(Value::as_str)!=Some("token_count"){return Ok(());}
                let Some(usage)=v.pointer("/payload/info/total_token_usage").filter(|u|u.is_object())else{return Ok(());};
                if !self.identity{return Err("identity_mismatch".into());}
                let(day,time)=timestamp(v.get("timestamp").ok_or("unsupported")?)?;
                let totals=UsageTotals{input_tokens:number(usage,"input_tokens")?,output_tokens:number(usage,"output_tokens")?,cached_input_tokens:number(usage,"cached_input_tokens")?,cache_creation_input_tokens:None,total_tokens:number(usage,"total_tokens")?};
                if !totals.any(){return Ok(());}
                if matches!((totals.cached_input_tokens,totals.input_tokens),(Some(c),Some(i))if c>i){return Err("unsupported".into());}
                if let(Some(i),Some(o),Some(t))=(totals.input_tokens,totals.output_tokens,totals.total_tokens){if add(Some(i),Some(o))?!=Some(t){return Err("unsupported".into());}}
                if !self.seen.insert((time.clone(),totals.values())){return Ok(());}
                let delta=if let Some(prior)=&self.previous{totals.delta(prior)?}else{totals.clone()};
                if self.daily.len()>=4096&&!self.daily.contains_key(&day){return Err("too_large".into());}
                if delta.values().iter().any(|n|n.is_some_and(|n|n>0))||self.previous.is_none(){
                    let sum=if let Some(old)=self.daily.get(&day){old.sum(&delta)?}else{delta};self.daily.insert(day,sum);
                }
                self.previous=Some(totals);self.measured=Some(time);
            }
        }Ok(())
    }
    fn finish(mut self,partial:bool)->Result<Parsed>{
        let totals=if self.session.provider==Provider::Claude{
            let mut total:Option<UsageTotals>=None;
            let outputs=std::mem::take(&mut self.outputs);
            for(id, (day,time,mut counts))in self.messages{
                if let Some(o)=outputs.get(&id){counts.output_tokens=Some(*o);counts.total_tokens=add(counts.input_tokens,Some(*o))?;}
                if self.daily.len()>=4096&&!self.daily.contains_key(&day){return Err("too_large".into());}
                total=Some(if let Some(old)=total{old.sum(&counts)?}else{counts.clone()});
                let sum=if let Some(old)=self.daily.get(&day){old.sum(&counts)?}else{counts};self.daily.insert(day,sum);
                if self.measured.as_ref().is_none_or(|old|time>*old){self.measured=Some(time);}
            }total.unwrap_or_default()
        }else{self.previous.unwrap_or_default()};
        let partial=partial||totals.input_tokens.is_none()||totals.output_tokens.is_none()
            ||totals.cached_input_tokens.is_none()||totals.total_tokens.is_none();
        Ok(Parsed{totals,daily:self.daily.into_iter().map(|(date,totals)|UsageDay{date,totals}).collect(),measured:self.measured,partial})
    }
}
