//! §34/§35 — the Native Messaging host: the browser's only legal door into the workbench.
//!
//! §35 freezes the chain (`Content Script → Service Worker → Schema Validation →
//! Native Messaging → Desktop Core`) and names the five things that must be verified:
//! `origin`, `command`, `payload`, `size`, `project scope`. This module is where the
//! `Desktop Core` end of that chain exists, and it is deliberately the *only* place in
//! the workspace that speaks the wire format.
//!
//! ## The frame
//!
//! Chrome's Native Messaging is length-prefixed JSON: four little-endian bytes, then that
//! many bytes of UTF-8. [`write_frame`] / [`read_frame`] implement it. The size check is
//! the first of §35's five and lives in the codec on purpose: an oversized frame is
//! refused before a single byte of JSON is parsed, so a hostile or buggy extension cannot
//! make the workbench allocate a 4 GiB buffer out of four header bytes.
//!
//! ## The protocol
//!
//! `codlas-nm/1`. One request, one reply, never a push — the host has no way to open a
//! channel to a tab on its own, and pretending otherwise would be a silent second
//! transport. Four commands, and §34's list of extension duties maps onto them exactly:
//!
//! ```text
//! hello    what can this pair do?          (identify the source, §37 capabilities)
//! capture  import new messages             (§12-§14 → §27 judge → §51 cursor-free push)
//! open     bring the target page to front   (host replies with an action, extension acts)
//! inject   put a pack in the composer       (fills, never sends — §7.8)
//! ```
//!
//! The last two are the honest shape of §34's "打开目标网页 / 准备输入内容": the host cannot
//! touch a browser, so `open` and `inject` answer with an *instruction for the extension*
//! (`{"action":"openTab"…}`, `{"action":"fillComposer","send":false}`). §37's
//! `AUTO_EXECUTE` is not claimed here either — the reply says "put this in front of the
//! user", and sending stays a human action (§7.8).
//!
//! ## Trust, stated plainly
//!
//! The real authorisation is Chrome's: a native host is reachable only by the extension
//! ids listed in the manifest written by [`HostManifest::render`]. The `origin` field is
//! the extension *reporting* which tab the material came from, and [`HostPolicy`] checks
//! it as §35 demands — a consistency gate, not a security boundary. [`HostPolicy`] also
//! documents what follows from that: nothing here is a secret store, and §53 keeps
//! secrets out of the payload path entirely.

use std::io::{BufRead, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vibe_core::error::{CoreError, Result};
use vibe_domain::{Message, MessageRole, NormalizedConversationBatch, SourceType};
use chrono::{DateTime, Utc};

use crate::adapter::blank_batch;
use crate::pipeline::IngestPipeline;

/// The frozen wire version (§34/§35). A request that says anything else is refused.
pub const PROTOCOL: &str = "codlas-nm/1";

/// Chrome's own host→extension limit, used as the ceiling for a frame length we will
/// even allocate for.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// What one `capture`/`inject` body may weigh. A long AI thread is tens of kilobytes;
/// anything past this is either a bug or an attempt to make the workbench parse a
/// megabyte per keystroke.
pub const MAX_PAYLOAD_BYTES: usize = 245_760;

/// The assistant products whose pages `capture` may claim to come from without the user
/// having added the origin by hand (§38.2 "Generic Web", §38.3 "ChatGPT Web").
pub const DEFAULT_ALLOWED_ORIGINS: &[&str] = &[
    "https://chatgpt.com",
    "https://*.chatgpt.com",
    "https://chat.openai.com",
    "https://claude.ai",
    "https://gemini.google.com",
];

// --------------------------------------------------------------------------- frames

/// Write one length-prefixed JSON frame. `json` must already be serialised — this is the
/// codec, not a marshaller, so a caller cannot accidentally send a double-encoded string.
pub fn write_frame<W: Write>(writer: &mut W, json: &str) -> Result<()> {
    let bytes = json.as_bytes();
    let length = u32::try_from(bytes.len()).map_err(|_| {
        CoreError::Validation(format!("size rejected: reply is {} bytes", bytes.len()))
    })?;
    if length as usize > MAX_FRAME_BYTES {
        return Err(CoreError::Validation(format!(
            "size rejected: reply is {} bytes, over the {MAX_FRAME_BYTES} frame limit",
            bytes.len()
        )));
    }
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()?;
    Ok(())
}

/// Read one frame. `Ok(None)` means the peer closed the channel cleanly, which is how
/// Chrome ends a session — not an error.
pub fn read_frame<R: BufRead>(reader: &mut R) -> Result<Option<String>> {
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(CoreError::Io(format!("frame header: {error}"))),
    }
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(CoreError::Validation(format!(
            "size rejected: frame declares {length} bytes, legal range is 1..={MAX_FRAME_BYTES}"
        )));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).map_err(|error| {
        CoreError::Io(format!("frame body truncated after {length} bytes: {error}"))
    })?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|error| CoreError::Encoding(format!("payload rejected: not UTF-8: {error}")))
}

/// The bytes a frame of `json` would occupy, header included.
pub fn frame_len(json: &str) -> usize {
    json.as_bytes().len() + 4
}

// -------------------------------------------------------------------------- requests

/// §34's four verbs, and nothing else. An unknown command is a refusal rather than a
/// guess, which is what keeps §35's `command` check meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostCommand {
    Hello,
    Capture,
    Open,
    Inject,
}

impl HostCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            HostCommand::Hello => "hello",
            HostCommand::Capture => "capture",
            HostCommand::Open => "open",
            HostCommand::Inject => "inject",
        }
    }

    pub const ALL: &'static [HostCommand] =
        &[HostCommand::Hello, HostCommand::Capture, HostCommand::Open, HostCommand::Inject];
}

/// The envelope every request carries. `project` and `payload` stay untyped here: §35
/// validates the command's own body per command, in [`HostPolicy::validate`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub protocol: String,
    pub origin: String,
    pub command: HostCommand,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

/// `capture` — one thread the user let the extension read (§34 "导入新消息").
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturePayload {
    /// Which `SourceType` row this lands under: `chatgpt_web` or `generic_web` (§12).
    pub source: String,
    /// The product's own thread id — the dedup key that makes a second push cheap (§52).
    pub conversation_id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub messages: Vec<Turn>,
}

/// One line of the thread. `role` deserialises straight into [`MessageRole`], so a turn
/// claimed to be from a role the domain does not have is a payload refusal.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub role: MessageRole,
    pub content: String,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub at: Option<DateTime<Utc>>,
}

/// `open` — the page the extension should bring to the front (§34 "打开目标网页").
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPayload {
    pub url: String,
}

/// `inject` — text to place in the composer (§34 "准备输入内容"). There is no `send`
/// field on purpose: sending is not this protocol's to ask for (§7.8).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectPayload {
    pub text: String,
}

// ------------------------------------------------------------------------------ policy

/// The §35 gate, held as data so `wp host serve --allow-origin` can widen it without a
/// code change. Two of the five checks are here (origin, command + payload + size);
/// `project scope` is [`NativeHost`]'s, because only the host holds the repositories.
#[derive(Debug, Clone)]
pub struct HostPolicy {
    pub protocol: String,
    /// Exact `https://host` origins, plus `https://*.host` for subdomains.
    pub allowed_origins: Vec<String>,
    pub max_payload_bytes: usize,
}

impl Default for HostPolicy {
    fn default() -> Self {
        Self {
            protocol: PROTOCOL.to_string(),
            allowed_origins: DEFAULT_ALLOWED_ORIGINS.iter().map(|s| s.to_string()).collect(),
            max_payload_bytes: MAX_PAYLOAD_BYTES,
        }
    }
}

impl HostPolicy {
    /// Add one origin the user consented to — §38.2's "Generic Web" over a page that is
    /// not a known assistant. Idempotent, so repeated flags do not duplicate.
    pub fn allow_origin(mut self, origin: &str) -> Self {
        let origin = origin.trim().trim_end_matches('/').to_string();
        if !origin.is_empty() && !self.allowed_origins.contains(&origin) {
            self.allowed_origins.push(origin);
        }
        self
    }

    /// Parse the frame and run §35's `origin`, `command`, `payload` and `size` checks.
    /// Every refusal names which check fired, because §62 Degradation has to be able to
    /// tell the user what went wrong without a log file.
    pub fn validate(&self, raw: &str) -> Result<Request> {
        if raw.len() > self.max_payload_bytes {
            return Err(CoreError::Validation(format!(
                "size rejected: request is {} bytes, over the {} limit",
                raw.len(),
                self.max_payload_bytes
            )));
        }
        let request: Request = serde_json::from_str(raw).map_err(|error| {
            CoreError::Validation(format!("command rejected: {error}"))
        })?;
        if request.protocol != self.protocol {
            return Err(CoreError::Validation(format!(
                "command rejected: protocol is {:?}, this host speaks {:?}",
                request.protocol, self.protocol
            )));
        }
        if !self.origin_accepted(request.command, &request.origin) {
            return Err(CoreError::Validation(format!(
                "origin rejected: {} is not in the allow-list ({}); pass --allow-origin {} to trust it",
                request.origin,
                self.allowed_origins.join(", "),
                request.origin
            )));
        }
        Self::check_payload(request.command, &request.payload, self.max_payload_bytes)?;
        Ok(request)
    }

    /// §35's `origin` check, split by what a command can carry. `capture` reads a page and
    /// `inject` types into one, so their origin has to be an allow-listed `https://host`.
    /// `hello` and `open` carry no page material — 自检 asks what this pair can do, and
    /// `open`'s target is held to the same list by [`NativeHost::open`] itself — so those two
    /// also accept the extension's own `chrome-extension://<id>` origin, which is what a popup
    /// with no tab to point at honestly has to say. That is not a claim taken on trust: Chrome
    /// only lets the ids in the manifest written by [`HostManifest::render`] reach this
    /// process, so an extension origin is a label the browser enforced upstream.
    fn origin_accepted(&self, command: HostCommand, origin: &str) -> bool {
        if origin_allowed(&self.allowed_origins, origin) {
            return true;
        }
        matches!(command, HostCommand::Hello | HostCommand::Open) && is_extension_origin(origin)
    }

    /// `payload`: the body has to match its command, and only its command — a `capture`
    /// shaped frame may not be answered by the `open` path even if it parses as both.
    fn check_payload(command: HostCommand, payload: &Value, max_bytes: usize) -> Result<()> {
        let body = serde_json::to_string(payload).unwrap_or_default();
        if body.len() > max_bytes {
            return Err(CoreError::Validation(format!(
                "size rejected: {} body is {} bytes, over the {} limit",
                command.as_str(),
                body.len(),
                max_bytes
            )));
        }
        match command {
            HostCommand::Hello => Ok(()),
            HostCommand::Capture => {
                let capture: CapturePayload = from_payload(payload)?;
                if capture.conversation_id.trim().is_empty() {
                    return Err(CoreError::Validation(
                        "payload rejected: conversationId is required".into(),
                    ));
                }
                if capture.messages.is_empty() {
                    return Err(CoreError::Validation(
                        "payload rejected: capture carried no messages".into(),
                    ));
                }
                let source = SourceType::parse(&capture.source).ok_or_else(|| {
                    CoreError::Validation(format!(
                        "payload rejected: unknown source {:?}",
                        capture.source
                    ))
                })?;
                if !matches!(source, SourceType::ChatgptWeb | SourceType::GenericWeb) {
                    return Err(CoreError::Validation(format!(
                        "payload rejected: source {:?} is not a web source — this host carries chatgpt_web or generic_web",
                        capture.source
                    )));
                }
                for (index, turn) in capture.messages.iter().enumerate() {
                    if turn.content.trim().is_empty() {
                        return Err(CoreError::Validation(format!(
                            "payload rejected: message {index} is empty"
                        )));
                    }
                }
                Ok(())
            }
            HostCommand::Open => {
                let open: OpenPayload = from_payload(payload)?;
                url_host(&open.url).ok_or_else(|| {
                    CoreError::Validation(format!(
                        "payload rejected: url {:?} is not an absolute http(s) URL",
                        open.url
                    ))
                })?;
                Ok(())
            }
            HostCommand::Inject => {
                let inject: InjectPayload = from_payload(payload)?;
                if inject.text.trim().is_empty() {
                    return Err(CoreError::Validation(
                        "payload rejected: inject carried no text".into(),
                    ));
                }
                Ok(())
            }
        }
    }
}

fn from_payload<T: DeserializeOwned>(payload: &Value) -> Result<T> {
    serde_json::from_value(payload.clone())
        .map_err(|error| CoreError::Validation(format!("payload rejected: {error}")))
}

/// `https://host` equality, with `*.host` meaning "that host or any subdomain of it".
/// Rejects anything that is not a bare https origin, so a path or a query in the field is
/// a refusal rather than a match nobody intended.
fn origin_allowed(patterns: &[String], origin: &str) -> bool {
    let Some(host) = origin_host(origin) else {
        return false;
    };
    patterns.iter().any(|pattern| {
        let Some(pattern_host) = origin_host(pattern) else {
            return false;
        };
        match pattern_host.strip_prefix("*.") {
            Some(suffix) => host == suffix || host.ends_with(&format!(".{suffix}")),
            None => host == pattern_host,
        }
    })
}

/// The host of a bare `https://host` string, or `None`.
fn origin_host(origin: &str) -> Option<&str> {
    let rest = origin.strip_prefix("https://")?;
    let host = rest.strip_suffix('/').unwrap_or(rest);
    (!host.is_empty() && !host.contains('/')).then_some(host)
}

/// `chrome-extension://<id>`, with the trailing slash Chrome's own URLs carry tolerated and a
/// path rejected — the shape [`HostManifest`] lists extension ids in, and the shape the
/// service worker reports for itself.
fn is_extension_origin(origin: &str) -> bool {
    let Some(rest) = origin.strip_prefix("chrome-extension://") else {
        return false;
    };
    let id = rest.strip_suffix('/').unwrap_or(rest);
    !id.is_empty() && !id.contains('/')
}

/// The host of an `http(s)` URL, used to hold `open` to the same page list as `capture`.
fn url_host(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|host| !host.is_empty())?;
    Some(host)
}

// ------------------------------------------------------------------------------- host

/// The dispatch end of the channel: policy in, replies out. Borrows the pipeline because
/// `capture` is a write and the pipeline is the only thing allowed to make one (§7.6).
pub struct NativeHost<'a> {
    pipeline: &'a IngestPipeline,
    policy: HostPolicy,
    default_project: Option<String>,
}

impl<'a> NativeHost<'a> {
    pub fn new(pipeline: &'a IngestPipeline, policy: HostPolicy) -> Self {
        Self {
            pipeline,
            policy,
            default_project: None,
        }
    }

    /// The project a frame with no `project` field is scoped to — `wp host serve
    /// --project`, so an extension never has to know an id.
    pub fn for_project(mut self, project: Option<String>) -> Self {
        self.default_project = project.filter(|id| !id.trim().is_empty());
        self
    }

    pub fn policy(&self) -> &HostPolicy {
        &self.policy
    }

    /// Serve until the peer closes stdin. A refused frame replies and keeps the session
    /// alive: one bad message must not end the run in silence (§62 Degradation).
    pub fn serve<R: BufRead, W: Write>(&self, reader: &mut R, writer: &mut W) -> Result<usize> {
        let mut handled = 0;
        loop {
            match read_frame(reader)? {
                None => return Ok(handled),
                Some(raw) => {
                    write_frame(writer, &self.handle(&raw))?;
                    handled += 1;
                }
            }
        }
    }

    /// One frame in, one reply out. This never returns `Err`: the reply envelope carries
    /// the refusal, so a caller cannot lose the reason by dropping a `Result`.
    pub fn handle(&self, raw: &str) -> String {
        match self.dispatch(raw) {
            Ok(data) => json!({ "ok": true, "data": data }).to_string(),
            Err(error) => json!({ "ok": false, "error": error.to_string() }).to_string(),
        }
    }

    fn dispatch(&self, raw: &str) -> Result<Value> {
        let request = self.policy.validate(raw)?;
        match request.command {
            HostCommand::Hello => Ok(self.hello(request.project.as_deref())),
            command => {
                let project = self.scoped_project(request.project.as_deref())?;
                match command {
                    HostCommand::Capture => self.capture(&project, &request),
                    HostCommand::Open => self.open(&request),
                    HostCommand::Inject => self.inject(&request),
                    HostCommand::Hello => unreachable!("hello answers above"),
                }
                .map(|mut value| {
                    if let Some(object) = value.as_object_mut() {
                        object.insert("project".into(), json!(project));
                    }
                    value
                })
            }
        }
    }

    /// §35 `project scope`: a frame either names a project this workbench has, or is
    /// scoped by the `--project` the host was started with. Never "whichever one exists",
    /// because a browser push landing in the wrong project is worse than a refusal.
    fn scoped_project(&self, explicit: Option<&str>) -> Result<String> {
        let id = explicit
            .map(str::to_string)
            .or_else(|| self.default_project.clone())
            .ok_or_else(|| {
                CoreError::Validation(
                    "project scope rejected: no project in the request and the host was started without --project".into(),
                )
            })?;
        if self
            .pipeline
            .repositories()
            .projects
            .get_project(&id)?
            .is_some()
        {
            return Ok(id);
        }
        Err(CoreError::Validation(format!(
            "project scope rejected: this workbench has no project {id}"
        )))
    }

    fn hello(&self, explicit: Option<&str>) -> Value {
        json!({
            "protocol": self.policy.protocol,
            "commands": HostCommand::ALL.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
            "capabilities": ["read_history", "read_current_thread", "open_app", "inject_text"],
            "allowedOrigins": &self.policy.allowed_origins,
            "project": explicit.or(self.default_project.as_deref()),
            "autoExecute": false
        })
    }

    /// The whole point of §34: a thread the user allowed the extension to read becomes
    /// Source / Conversation / Message rows, then judged events. No cursor moves — the
    /// push path is idempotent through §52's dedup, and the reply reports what landed so
    /// the popup can say "3 new, 12 already here" instead of a spinner.
    fn capture(&self, project: &str, request: &Request) -> Result<Value> {
        let payload: CapturePayload = from_payload(&request.payload)?;
        let batch = capture_batch(&payload);
        let report = self.pipeline.ingest_messages(project, &batch)?;
        Ok(json!({
            "action": "captured",
            "sourceId": report.source_id,
            "messages": {
                "inserted": report.messages_inserted,
                "duplicate": report.messages_duplicate
            },
            "events": {
                "appended": report.events_appended,
                "pending": report.events_pending,
                "candidate": report.events_candidate
            },
            "nearDuplicates": report.candidates.len()
        }))
    }

    /// `open` is a request, not an act: the host cannot reach a browser, so it answers
    /// with the instruction the service worker can carry out. The URL is held to the same
    /// origin list as `capture`, so a prepared pack cannot point the user at a lookalike.
    fn open(&self, request: &Request) -> Result<Value> {
        let payload: OpenPayload = from_payload(&request.payload)?;
        let host = url_host(&payload.url).unwrap_or_default();
        if !origin_allowed(&self.policy.allowed_origins, &format!("https://{host}")) {
            return Err(CoreError::Validation(format!(
                "origin rejected: open target {host} is not in the allow-list"
            )));
        }
        Ok(json!({ "action": "openTab", "url": payload.url }))
    }

    /// `inject` fills; it never submits. §37's `AUTO_EXECUTE` is not claimed anywhere in
    /// V1, and the reply says so in the same words the CLI prints.
    fn inject(&self, request: &Request) -> Result<Value> {
        let payload: InjectPayload = from_payload(&request.payload)?;
        Ok(json!({
            "action": "fillComposer",
            "text": payload.text,
            "send": false,
            "note": "the workbench fills the composer; sending stays your action"
        }))
    }
}

/// Turn a `capture` body into the batch shape the pipeline stores. Identity fields stay
/// blank for the pipeline to derive, except `source_id`, which carries the `SourceType`
/// slug — that is how [`IngestPipeline::ingest_messages`] knows which source type row the
/// material belongs to, and it is why `payload.source` is validated before it is used.
pub fn capture_batch(payload: &CapturePayload) -> NormalizedConversationBatch {
    let observed_at = Utc::now();
    let messages: Vec<Message> = payload
        .messages
        .iter()
        .map(|turn| {
            let mut message = Message::new(
                String::new(),
                turn.role,
                turn.content.clone(),
                turn.message_id.clone(),
                turn.at.unwrap_or(observed_at),
            );
            message.created_at = turn.at;
            message
        })
        .collect();
    let mut batch = blank_batch(
        Some(payload.conversation_id.clone()),
        payload.title.clone(),
        messages,
        None,
    );
    batch.conversation.source_id = payload.source.clone();
    batch
}

// ----------------------------------------------------------------------------- manifest

/// The name Chrome keys the host record on. One name, one JSON file in
/// `NativeMessagingHosts`, so an upgrade overwrites its own record and nothing else.
pub const HOST_NAME: &str = "app.codlas.workbench.host";

/// Chrome's `NativeMessagingHosts` record. Its field names are Chrome's, not ours, so
/// `allowed_origins` keeps the snake_case spelling here while everything on the wire is
/// camelCase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostManifest {
    pub name: String,
    pub description: String,
    /// Absolute path to what Chrome launches. Chrome passes no arguments, so this is a
    /// wrapper script that carries `wp host serve --project …`, not the bare binary.
    pub path: String,
    #[serde(rename = "type")]
    pub transport: String,
    #[serde(rename = "allowed_origins")]
    pub allowed_origins: Vec<String>,
}

impl HostManifest {
    /// Build the record for one host path and a set of extension ids. An id already
    /// written as a full `chrome-extension://…/` origin is taken as given, so a caller can
    /// paste what `chrome://extensions` shows either way.
    pub fn new(path: impl Into<String>, extension_ids: &[String]) -> Self {
        Self {
            name: HOST_NAME.to_string(),
            description: "Codlas workbench — browser capture and context hand-off".to_string(),
            path: path.into(),
            transport: "stdio".to_string(),
            allowed_origins: extension_ids
                .iter()
                .map(|id| extension_origin(id))
                .collect(),
        }
    }

    /// The file's contents. Pretty, because a person reads this file when a connection
    /// refuses and `jq` is not always at hand.
    pub fn render(&self) -> String {
        serde_json::to_string_pretty(self).expect("manifest is serialisable by construction")
    }
}

fn extension_origin(id: &str) -> String {
    let id = id.trim();
    if id.starts_with("chrome-extension://") {
        return if id.ends_with('/') {
            id.to_string()
        } else {
            format!("{id}/")
        };
    }
    format!("chrome-extension://{id}/")
}
