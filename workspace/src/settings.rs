//! Validated application preferences only. No provider configuration, credentials,
//! filesystem destinations, model or permission flags are part of this schema.
use super::{db,Result,Store};
use rusqlite::{params,Connection,OptionalExtension,Transaction,TransactionBehavior};
use serde::{Deserialize,Serialize};
use std::collections::HashSet;

const FORMAT:i64=1;
const MAX_JSON:i64=256*1024;
const MAX_REVISION:u64=9_007_199_254_740_991;

#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuickPrompt { pub id:String,pub label:String,pub prompt:String }

#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSettings {
    pub theme:String,
    pub ui_scale_percent:u16,
    pub density:String,
    pub default_page:String,
    pub preferred_provider:String,
    pub remember_filters:bool,
    #[serde(default)]
    pub demo_mode:bool,
    /// "stop" (default) stops every session when the window closes; "background" hides the window and keeps running sessions.
    #[serde(default="default_close_behavior")]
    pub close_behavior:String,
    pub hierarchy_provider:String,
    pub hierarchy_project:String,
    pub hierarchy_status:String,
    pub hierarchy_view:String,
    pub live_provider_filter:String,
    pub usage_provider:String,
    pub usage_period:String,
    pub terminal_font_size:u8,
    pub terminal_cursor_blink:bool,
    pub terminal_scrollback:u32,
    pub usage_refresh_seconds:u16,
    pub quick_prompts:Vec<QuickPrompt>,
}
fn default_close_behavior()->String{"stop".into()}
impl Default for AppSettings {
    fn default()->Self{Self{
        theme:"dark".into(),ui_scale_percent:100,density:"comfortable".into(),default_page:"sessions".into(),preferred_provider:"claude".into(),
        remember_filters:false,demo_mode:false,close_behavior:default_close_behavior(),hierarchy_provider:String::new(),hierarchy_project:"Agent App".into(),hierarchy_status:String::new(),hierarchy_view:"tree".into(),
        live_provider_filter:String::new(),usage_provider:String::new(),usage_period:"all".into(),terminal_font_size:13,terminal_cursor_blink:true,
        terminal_scrollback:2000,usage_refresh_seconds:0,quick_prompts:vec![
            QuickPrompt{id:"inbox".into(),label:"Review shared inbox".into(),prompt:"Review the shared inbox and summarize new items, decisions needed, and next actions.".into()},
            QuickPrompt{id:"status".into(),label:"Summarize status".into(),prompt:"Summarize the current project status, completed work, blockers, and the next concrete step.".into()},
            QuickPrompt{id:"handoff".into(),label:"Prepare handoff".into(),prompt:"Prepare a concise handoff with the current state, evidence, open issues, and next actions. Follow the existing project rules.".into()},
        ],
    }}
}

#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize)]
#[serde(rename_all="snake_case")]
pub enum SettingsRecovery { CorruptJson,InvalidSettings,NewerVersion,Oversized,InvalidMetadata }

#[derive(Debug,Clone,PartialEq,Eq,Serialize)]
pub struct SettingsSnapshot {
    pub settings:AppSettings,
    pub defaults:AppSettings,
    pub revision:u64,
    pub recovery:Option<SettingsRecovery>,
    pub writable:bool,
}

#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope { version:u32,settings:AppSettings }

/// Unlike Value, a map visitor observes every duplicate top-level version key.
/// Ambiguous future/current payloads must not be downgraded by last-key wins.
struct FutureVersion(bool);
impl<'de> Deserialize<'de> for FutureVersion {
    fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->std::result::Result<Self,D::Error>{
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value=FutureVersion;
            fn expecting(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str("a settings envelope")}
            fn visit_map<M:serde::de::MapAccess<'de>>(self,mut map:M)->std::result::Result<Self::Value,M::Error>{
                let mut future=false;
                while let Some(key)=map.next_key::<String>()?{
                    if key=="version"{
                        let version:serde_json::Value=map.next_value()?;
                        future|=version.as_u64().is_some_and(|v|v>FORMAT as u64)
                            ||version.as_f64().is_some_and(|v|v>FORMAT as f64);
                    }else{map.next_value::<serde::de::IgnoredAny>()?;}
                }Ok(FutureVersion(future))
            }
        }deserializer.deserialize_map(Visitor)
    }
}

fn plain_text(value:&str)->bool{
    !value.chars().any(|c|c.is_control()||matches!(c,'\u{2028}'|'\u{2029}'|'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{feff}'|'\u{200b}'|'\u{2060}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}'))
}
impl AppSettings {
    pub fn validate(&self)->Result<()>{
        let one_of=|s:&str,allowed:&[&str]|allowed.contains(&s);
        if !one_of(&self.theme,&["dark","light","system","papercolor-light","papercolor-dark"])||!(80..=150).contains(&self.ui_scale_percent)
            ||!one_of(&self.density,&["comfortable","compact"])||!one_of(&self.default_page,&["agents","sessions","usage","settings"])
            ||!one_of(&self.preferred_provider,&["claude","codex"])||!one_of(&self.close_behavior,&["stop","background"])||!one_of(&self.hierarchy_provider,&["","claude","codex"])
            ||!one_of(&self.hierarchy_project,&["","Agent App","Documentation"])||!one_of(&self.hierarchy_status,&["","idle","busy","paused"])
            ||!one_of(&self.hierarchy_view,&["tree","project"])||!one_of(&self.live_provider_filter,&["","claude","codex"])
            ||!one_of(&self.usage_provider,&["","claude","codex"])||!one_of(&self.usage_period,&["all","today","7","30"])
            ||!(10..=24).contains(&self.terminal_font_size)||self.terminal_scrollback>10000
            ||![0,60,300,900].contains(&self.usage_refresh_seconds)||self.quick_prompts.len()>24{
            return Err("Invalid application settings".into());
        }
        let mut ids=HashSet::new();
        for preset in &self.quick_prompts{
            let mut bytes=preset.id.bytes();
            if preset.id.len()>40||!bytes.next().is_some_and(|c|c.is_ascii_lowercase())
                ||!bytes.all(|c|c.is_ascii_lowercase()||c.is_ascii_digit()||c==b'_'||c==b'-')
                ||!ids.insert(preset.id.as_str())||preset.label.trim().is_empty()||preset.label.chars().count()>48
                ||preset.prompt.trim().is_empty()||preset.prompt.chars().count()>4000
                ||!plain_text(&preset.label)||!plain_text(&preset.prompt){return Err("Invalid quick prompt settings".into());}
        }Ok(())
    }
}

pub(super) fn initialize(tx:&Transaction<'_>)->Result<()>{
    db(tx.execute_batch("CREATE TABLE IF NOT EXISTS app_settings(
        singleton INTEGER PRIMARY KEY CHECK(singleton=1),revision INTEGER NOT NULL CHECK(revision>0),
        format_version INTEGER NOT NULL,settings_json TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS app_settings_recovery(
        revision INTEGER PRIMARY KEY,format_version INTEGER NOT NULL,settings_json BLOB NOT NULL);
        PRAGMA user_version=2;"))
}
fn fallback(revision:u64,recovery:SettingsRecovery,writable:bool)->SettingsSnapshot{
    SettingsSnapshot{settings:AppSettings::default(),defaults:AppSettings::default(),revision,recovery:Some(recovery),writable}
}

/// One SELECT returns row metadata and a bounded byte payload atomically. Large
/// externally modified rows never get materialized in application memory.
fn snapshot(connection:&Connection)->Result<SettingsSnapshot>{
    let row:Option<(Option<i64>,Option<i64>,String,Option<i64>,Option<Vec<u8>>)>=db(connection.query_row(
        "SELECT CASE WHEN typeof(revision)='integer' THEN revision END,
         CASE WHEN typeof(format_version)='integer' THEN format_version END,
         typeof(settings_json),length(CAST(settings_json AS BLOB)),
         CASE WHEN typeof(settings_json)='text' AND length(CAST(settings_json AS BLOB))<=?1 THEN CAST(settings_json AS BLOB) END
         FROM app_settings WHERE singleton=1",[MAX_JSON],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional())?;
    let Some((revision,format,kind,bytes,raw))=row else{return Ok(SettingsSnapshot{settings:AppSettings::default(),defaults:AppSettings::default(),revision:0,recovery:None,writable:true});};
    let revision=match revision.and_then(|n|u64::try_from(n).ok()).filter(|n|*n>0&&*n<=MAX_REVISION){
        Some(n)=>n,None=>return Ok(fallback(0,SettingsRecovery::InvalidMetadata,false)),
    };
    if format.is_some_and(|n|n>FORMAT){return Ok(fallback(revision,SettingsRecovery::NewerVersion,false));}
    if format!=Some(FORMAT)||kind!="text"{return Ok(fallback(revision,SettingsRecovery::InvalidMetadata,false));}
    if bytes.is_none_or(|n|n<0||n>MAX_JSON){return Ok(fallback(revision,SettingsRecovery::Oversized,false));}
    let Some(raw)=raw else{return Ok(fallback(revision,SettingsRecovery::InvalidMetadata,false));};
    if serde_json::from_slice::<serde_json::Value>(&raw).is_err(){return Ok(fallback(revision,SettingsRecovery::CorruptJson,true));}
    if serde_json::from_slice::<FutureVersion>(&raw).is_ok_and(|v|v.0){return Ok(fallback(revision,SettingsRecovery::NewerVersion,false));}
    // Deserialize directly from the bytes: the strict derives reject duplicate
    // keys at every typed level rather than accepting Value's last-key wins.
    let envelope:Envelope=match serde_json::from_slice(&raw){Ok(value)=>value,Err(_)=>return Ok(fallback(revision,SettingsRecovery::InvalidSettings,true))};
    if envelope.version!=FORMAT as u32||envelope.settings.validate().is_err(){return Ok(fallback(revision,SettingsRecovery::InvalidSettings,true));}
    Ok(SettingsSnapshot{settings:envelope.settings,defaults:AppSettings::default(),revision,recovery:None,writable:true})
}

impl Store {
    /// App-owned SQLite only; loading never launches or rewrites a provider.
    pub fn settings_snapshot(&self)->Result<SettingsSnapshot>{snapshot(&self.connection)}
    pub fn settings_save(&mut self,expected_revision:u64,settings:AppSettings)->Result<SettingsSnapshot>{
        settings.validate()?;
        self.write_settings(expected_revision,settings,false)
    }
    pub fn settings_reset(&mut self,expected_revision:u64)->Result<SettingsSnapshot>{
        self.write_settings(expected_revision,AppSettings::default(),true)
    }
    fn write_settings(&mut self,expected_revision:u64,settings:AppSettings,reset:bool)->Result<SettingsSnapshot>{
        if expected_revision>=MAX_REVISION{return Err("Settings revision is outside the supported range".into());}
        let tx=db(self.connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current=snapshot(&tx)?;
        if current.revision!=expected_revision{return Err("Settings changed in another view; reload before saving".into());}
        if !current.writable{return Err("Stored settings require manual recovery; refusing to overwrite an unsupported format".into());}
        if current.recovery.is_some()&&!reset{return Err("Stored settings need recovery; review the warning and use Reset explicitly".into());}
        if current.recovery.is_some(){
            db(tx.execute("INSERT INTO app_settings_recovery(revision,format_version,settings_json)
                SELECT revision,format_version,CAST(settings_json AS BLOB) FROM app_settings WHERE singleton=1",[]))?;
        }
        let encoded=serde_json::to_string(&Envelope{version:FORMAT as u32,settings}).map_err(|_|"Cannot encode application settings")?;
        if encoded.len()>MAX_JSON as usize{return Err("Application settings exceed the bounded storage limit".into());}
        let revision=expected_revision+1;
        let changed=if expected_revision==0{
            db(tx.execute("INSERT INTO app_settings VALUES(1,?1,?2,?3)",params![revision,FORMAT,encoded]))?
        }else{
            db(tx.execute("UPDATE app_settings SET revision=?1,format_version=?2,settings_json=?3 WHERE singleton=1 AND revision=?4",params![revision,FORMAT,encoded,expected_revision]))?
        };
        if changed!=1{return Err("Settings changed in another view; reload before saving".into());}
        let saved=snapshot(&tx)?;
        db(tx.commit())?;
        Ok(saved)
    }
}
