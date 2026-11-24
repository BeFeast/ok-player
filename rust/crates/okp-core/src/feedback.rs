//! Anonymous tester feedback using pomoi's app-scoped multipart API.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

const ENDPOINT: &str = "https://pomoi.co/api/feedback/ok-player";
const ISSUE_BASE: &str = "https://git.oklabs.uk/BeFeast/ok-player/issues/";
const MAX_SCREENSHOT: usize = 8 * 1024 * 1024;
const MAX_STATE: u64 = 12 * 1024 * 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

pub const PRIVACY: &str = "Your description, app and OS details, and any screenshot will be public in the OK Player issue tracker. Name, installation ID and diagnostics are private.";
const UNCERTAIN: &str =
    "Delivery could not be confirmed. Check the OK Player issue tracker before retrying.";
const LOCAL_ERROR: &str = "Could not save or read feedback on this device.";
const BUSY: &str = "Feedback is being sent from another window.";
const STALE: &str = "The saved feedback has changed. Reopen Feedback before trying again.";

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Bug,
    Feature,
}

/// Deliberately limited to build and platform data. Playback details are private.
#[derive(Clone, Deserialize, Serialize)]
pub struct Metadata {
    pub version: String,
    pub build: String,
    pub source: String,
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub channel: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Diagnostics {
    pub playback: Playback,
    pub fullscreen: bool,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Playback {
    Idle,
    Loading,
    Playing,
    Paused,
    Buffering,
    Ended,
    Error,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Report {
    pub text: String,
    pub kind: Kind,
    pub reporter_name: String,
    pub meta: Metadata,
    pub diagnostics: Option<Diagnostics>,
    /// Base64 keeps the PNG portable over the JSON bridge and in the saved draft.
    pub screenshot: Option<String>,
}

impl Report {
    pub fn encode_screenshot(png: &[u8]) -> Result<String, &'static str> {
        validate_png(png)?;
        Ok(STANDARD.encode(png))
    }

    pub fn screenshot_bytes(&self) -> Result<Option<Vec<u8>>, &'static str> {
        self.screenshot.as_deref().map(decode_png).transpose()
    }
}

fn validate_png(png: &[u8]) -> Result<(), &'static str> {
    if png.len() > MAX_SCREENSHOT {
        Err(
            "The screenshot exceeds 8 MiB. Remove it or resize the player window and capture again.",
        )
    } else if !png.starts_with(PNG_SIGNATURE) {
        Err("The screenshot is not a PNG.")
    } else {
        Ok(())
    }
}

fn decode_png(encoded: &str) -> Result<Vec<u8>, &'static str> {
    if encoded.len() > MAX_SCREENSHOT.div_ceil(3) * 4 {
        return Err("The screenshot exceeds 8 MiB.");
    }
    let png = STANDARD
        .decode(encoded)
        .map_err(|_| "The screenshot is not a PNG.")?;
    validate_png(&png)?;
    Ok(png)
}

#[derive(Deserialize)]
pub struct Submission {
    pub token: Option<Uuid>,
    pub report: Report,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Draft {
    pub token: Uuid,
    pub report: Report,
    pub message: String,
}

#[derive(Default, Deserialize, Serialize)]
struct State {
    install_id: Option<Uuid>,
    reporter_name: String,
    draft: Option<Draft>,
}

#[derive(Serialize)]
pub struct Form {
    pub privacy: &'static str,
    pub reporter_name: String,
    pub draft: Option<Draft>,
}

#[derive(Deserialize, Serialize)]
pub struct Receipt {
    pub issue_number: u64,
    pub issue_url: String,
}

pub struct Outbox {
    path: PathBuf,
    endpoint: String,
}

impl Outbox {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            path: config_dir.join("feedback.json"),
            endpoint: ENDPOINT.into(),
        }
    }

    /// Use a separate queue for the loopback verifier. Production reports always use pomoi.
    pub fn loopback(directory: &Path, port: u16) -> Self {
        Self {
            path: directory.join("feedback-test.json"),
            endpoint: format!("http://127.0.0.1:{port}/api/feedback/ok-player"),
        }
    }

    pub fn form(&self) -> Result<Form, &'static str> {
        let state = self.read()?;
        Ok(Form {
            privacy: PRIVACY,
            reporter_name: state.reporter_name,
            draft: state.draft,
        })
    }

    pub fn discard(&self, token: Uuid) -> Result<(), &'static str> {
        let _lock = self.lock()?;
        let mut state = self.read()?;
        if state.draft.as_ref().map(|draft| draft.token) != Some(token) {
            return Err(STALE);
        }
        state.draft = None;
        self.save(&state)
    }

    pub fn submit(&self, token: Option<Uuid>, report: Report) -> Result<Receipt, &'static str> {
        let screenshot = report.screenshot_bytes()?;
        if report.text.chars().count() > 10_000 || report.text.contains('\0') {
            return Err(
                "The description must contain at most 10,000 characters and no NUL characters.",
            );
        }
        let name = report.reporter_name.trim();
        if name.chars().count() > 128 || name.chars().any(char::is_control) {
            return Err("The name must contain at most 128 characters and no control characters.");
        }
        let meta =
            serde_json::to_vec(&report.meta).map_err(|_| "Could not prepare system details.")?;
        let log = report
            .diagnostics
            .as_ref()
            .and_then(|data| serde_json::to_vec_pretty(data).ok());
        let _lock = self.lock()?;
        let mut state = self.read()?;
        if state.draft.as_ref().map(|draft| draft.token) != token {
            return Err(STALE);
        }
        let install_id = *state.install_id.get_or_insert_with(Uuid::new_v4);
        state.reporter_name = name.to_owned();
        let (boundary, body) = multipart(
            &report,
            &meta,
            screenshot.as_deref(),
            log.as_deref(),
            install_id,
        );
        state.draft = Some(Draft {
            token: Uuid::new_v4(),
            report,
            message: UNCERTAIN.into(),
        });
        self.save(&state)?;
        match self.post(&boundary, &body) {
            Ok(receipt) => {
                state.draft = None;
                self.save(&state).map_err(|_| "The issue was created. Local cleanup failed. Check the issue tracker before retrying.")?;
                Ok(receipt)
            }
            Err(message) => {
                if let Some(draft) = &mut state.draft {
                    draft.message = message.into();
                }
                self.save(&state)?;
                Err(message)
            }
        }
    }

    fn read(&self) -> Result<State, &'static str> {
        match File::open(&self.path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(MAX_STATE + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| LOCAL_ERROR)?;
                if bytes.len() as u64 > MAX_STATE {
                    return Err(LOCAL_ERROR);
                }
                serde_json::from_slice(&bytes).map_err(|_| LOCAL_ERROR)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(_) => Err(LOCAL_ERROR),
        }
    }

    fn lock(&self) -> Result<File, &'static str> {
        let directory = self.path.parent().ok_or(LOCAL_ERROR)?;
        fs::create_dir_all(directory).map_err(|_| LOCAL_ERROR)?;
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.path.with_extension("lock"))
            .map_err(|_| LOCAL_ERROR)?;
        file.try_lock().map_err(|_| BUSY)?;
        Ok(file)
    }

    fn save(&self, state: &State) -> Result<(), &'static str> {
        let mut file = tempfile::NamedTempFile::new_in(self.path.parent().ok_or(LOCAL_ERROR)?)
            .map_err(|_| LOCAL_ERROR)?;
        serde_json::to_writer(&mut file, state).map_err(|_| LOCAL_ERROR)?;
        file.as_file().sync_all().map_err(|_| LOCAL_ERROR)?;
        file.persist(&self.path).map_err(|_| LOCAL_ERROR)?;
        Ok(())
    }

    fn post(&self, boundary: &str, body: &[u8]) -> Result<Receipt, &'static str> {
        let mut config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(45)))
            .max_redirects(0)
            .http_status_as_error(false);
        if self.endpoint.starts_with("http://127.0.0.1:") {
            config = config.proxy(None);
        }
        let response = ureq::Agent::new_with_config(config.build())
            .post(&self.endpoint)
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .send(body)
            .map_err(|_| UNCERTAIN)?;
        match response.status().as_u16() {
            201 => {
                let mut bytes = Vec::new();
                response
                    .into_body()
                    .into_reader()
                    .take(4097)
                    .read_to_end(&mut bytes)
                    .map_err(|_| UNCERTAIN)?;
                let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| UNCERTAIN)?;
                if bytes.len() > 4096
                    || receipt.issue_number == 0
                    || receipt.issue_url != format!("{ISSUE_BASE}{}", receipt.issue_number)
                {
                    return Err(UNCERTAIN);
                }
                Ok(receipt)
            }
            400 | 413 | 415 => Err(
                "The service rejected this report. Check the description and attachments before retrying.",
            ),
            404 => {
                Err("OK Player feedback is not available on the server yet. Your report was saved.")
            }
            429 => {
                Err("The feedback service is rate limited. Your report was saved. Try again later.")
            }
            503 => Err("The feedback service is temporarily unavailable. Your report was saved."),
            _ => Err(UNCERTAIN),
        }
    }
}

fn multipart(
    report: &Report,
    meta: &[u8],
    png: Option<&[u8]>,
    log: Option<&[u8]>,
    install_id: Uuid,
) -> (String, Vec<u8>) {
    let boundary = format!("okp-{}", Uuid::new_v4());
    let mut bytes = Vec::new();
    let mut part = |name: &str, file: Option<(&str, &str)>, data: &[u8]| {
        write!(
            bytes,
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\""
        )
        .expect("Vec write");
        if let Some((filename, mime)) = file {
            write!(bytes, "; filename=\"{filename}\"\r\nContent-Type: {mime}").expect("Vec write");
        }
        bytes.extend_from_slice(b"\r\n\r\n");
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(b"\r\n");
    };
    part("text", None, report.text.as_bytes());
    part(
        "kind",
        None,
        match report.kind {
            Kind::Bug => b"bug",
            Kind::Feature => b"fr",
        },
    );
    part("meta", None, meta);
    part("install_id", None, install_id.to_string().as_bytes());
    if !report.reporter_name.trim().is_empty() {
        part(
            "reporter_name",
            None,
            report.reporter_name.trim().as_bytes(),
        );
    }
    if let Some(png) = png {
        part("screenshot", Some(("screenshot.png", "image/png")), png);
    }
    if let Some(log) = log {
        part("log", Some(("log.txt", "text/plain")), log);
    }
    write!(bytes, "--{boundary}--\r\n").expect("Vec write");
    (boundary, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        net::TcpListener,
        sync::mpsc,
        thread,
    };

    fn report() -> Report {
        Report {
            text: "A synthetic playback report".into(),
            kind: Kind::Feature,
            reporter_name: "Example Reporter".into(),
            meta: Metadata {
                version: "0.11.0".into(),
                build: "702".into(),
                source: "0123456789abcdef0123456789abcdef01234567".into(),
                os: "linux".into(),
                os_version: "Debian 13".into(),
                arch: "x86_64".into(),
                channel: "preview".into(),
            },
            diagnostics: Some(Diagnostics {
                playback: Playback::Paused,
                fullscreen: false,
                width: 960,
                height: 580,
            }),
            screenshot: Some("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jZ0sAAAAASUVORK5CYII=".into()),
        }
    }

    fn fixture(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (
        u16,
        mpsc::Receiver<(String, Vec<u8>)>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            for (status, payload) in responses {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut input = BufReader::new(&mut socket);
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    assert!(input.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    headers.push_str(&line);
                }
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                let mut bytes = vec![0; length];
                input.read_exact(&mut bytes).unwrap();
                sender.send((headers, bytes)).unwrap();
                write!(
                    socket,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                )
                .unwrap();
            }
        });
        (port, receiver, worker)
    }

    fn field(headers: &str, body: &[u8], name: &str) -> Vec<u8> {
        let boundary = headers
            .lines()
            .find_map(|line| line.split_once("boundary=").map(|(_, value)| value))
            .unwrap();
        let line = format!("--{boundary}");
        let field = format!("name=\"{name}\"");
        // Find the header in the raw body so the screenshot's binary bytes stay intact.
        let start = body
            .windows(field.len())
            .position(|part| part == field.as_bytes())
            .unwrap();
        let offset = body[start..]
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + start
            + 4;
        let end = body[offset..]
            .windows(line.len())
            .position(|part| part == line.as_bytes())
            .unwrap()
            + offset;
        body[offset..end - 2].to_vec()
    }

    #[test]
    fn retries_only_on_request_and_keeps_private_data_out_of_metadata() {
        let (port, requests, worker) = fixture(vec![
            ("503 Service Unavailable", "{}"),
            (
                "201 Created",
                r#"{"issue_number":17,"issue_url":"https://git.oklabs.uk/BeFeast/ok-player/issues/17"}"#,
            ),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let outbox = Outbox::loopback(dir.path(), port);
        let report = report();
        let error = outbox.submit(None, report.clone()).err().unwrap();
        let (headers, body) = requests.recv().unwrap();
        assert!(headers.starts_with("POST /api/feedback/ok-player HTTP/1.1\r\n"));
        assert!(!headers.to_ascii_lowercase().contains("authorization:"));
        assert_eq!(field(&headers, &body, "kind"), b"fr");
        assert_eq!(field(&headers, &body, "text"), report.text.as_bytes());
        assert_eq!(
            field(&headers, &body, "reporter_name"),
            report.reporter_name.as_bytes()
        );
        let id = field(&headers, &body, "install_id");
        let meta: serde_json::Value =
            serde_json::from_slice(&field(&headers, &body, "meta")).unwrap();
        assert_eq!(meta["os"], "linux");
        assert_eq!(meta.as_object().unwrap().len(), 7);
        let log: serde_json::Value =
            serde_json::from_slice(&field(&headers, &body, "log")).unwrap();
        assert_eq!(log["playback"], "paused");
        assert_eq!(
            field(&headers, &body, "screenshot"),
            report.screenshot_bytes().unwrap().unwrap()
        );

        let reopened = Outbox::loopback(dir.path(), port);
        let draft = reopened.form().unwrap().draft.unwrap();
        assert_eq!(draft.message, error);
        assert_eq!(draft.report.text, report.text);
        assert_eq!(reopened.submit(None, report).err(), Some(STALE));
        assert!(requests.try_recv().is_err());
        let receipt = reopened.submit(Some(draft.token), draft.report).unwrap();
        assert_eq!(receipt.issue_number, 17);
        assert!(reopened.form().unwrap().draft.is_none());
        assert_eq!(reopened.form().unwrap().reporter_name, "Example Reporter");
        let (headers, body) = requests.recv().unwrap();
        assert_eq!(field(&headers, &body, "install_id"), id);
        worker.join().unwrap();
    }

    #[test]
    fn keeps_offline_report_until_discard_and_rejects_oversized_input() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let outbox = Outbox::loopback(dir.path(), port);
        let mut report = report();
        report.text = "a".repeat(10_001);
        assert!(outbox.submit(None, report.clone()).is_err());
        assert!(outbox.form().unwrap().draft.is_none());
        report.text = String::new();
        assert_eq!(outbox.submit(None, report).err(), Some(UNCERTAIN));
        let draft = outbox.form().unwrap().draft.unwrap();
        assert_eq!(draft.message, UNCERTAIN);
        outbox.discard(draft.token).unwrap();
        assert!(outbox.form().unwrap().draft.is_none());
    }
}
