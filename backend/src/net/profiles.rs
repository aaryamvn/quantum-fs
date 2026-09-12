//! Cosmetic display-name store, served beside the central directory on its own
//! line-oriented port. It holds no keys, admission state, member lists, or file
//! data: frame kinds 1-14, the handshake, and the directory's own request
//! handlers are untouched, exactly as the admin port leaves them untouched.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

use crate::{
    demo_log::{self, Kind},
    keystore::{atomic_private_write, read_private},
    Error, Result,
};

/// The admin port's conventions, so the two line protocols behave alike.
pub const PROFILE_IDLE_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_PROFILE_CONNECTIONS: usize = 64;
const MAX_PROFILE_LINE: usize = 512;
const MAX_REQUESTS_PER_CONNECTION: usize = 8;
const MAX_PROFILES: usize = 10_000;
const MAX_NAME_CHARS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Profile {
    name: String,
    updated_ms: u64,
}

/// Persistent `client_id -> display name` map. Cosmetic by decision
/// (`net-vault-join-directory.md`), so it is never an authority on identity.
pub struct ProfileStore {
    path: PathBuf,
    entries: BTreeMap<String, Profile>,
}

impl ProfileStore {
    /// A missing, unreadable, or corrupt file is never fatal: the directory
    /// keeps serving routes and the next PUT rewrites a well-formed store.
    pub fn open(path: &Path) -> Self {
        let entries = match read_private(path) {
            Ok(Some(bytes)) if !bytes.is_empty() => decode(bytes.as_slice()).unwrap_or_else(|| {
                report_unreadable(&format!("path  {}", path.display()));
                BTreeMap::new()
            }),
            Ok(_) => BTreeMap::new(),
            Err(error) => {
                report_unreadable(&format!("reason  {error}"));
                BTreeMap::new()
            }
        };
        Self {
            path: path.to_owned(),
            entries,
        }
    }

    pub fn get(&self, client_id: &str) -> Option<&str> {
        self.entries
            .get(client_id)
            .map(|profile| profile.name.as_str())
    }

    /// Writes the whole file atomically with owner-only permissions, then
    /// adopts the new map, so a failed write leaves memory and disk agreeing.
    pub fn put(&mut self, client_id: &str, name: &str) -> Result<()> {
        let mut entries = self.entries.clone();
        entries.insert(
            client_id.to_owned(),
            Profile {
                name: name.to_owned(),
                updated_ms: unix_millis(),
            },
        );
        while entries.len() > MAX_PROFILES {
            let oldest = entries
                .iter()
                .filter(|(id, _)| id.as_str() != client_id)
                .min_by_key(|(_, profile)| profile.updated_ms)
                .map(|(id, _)| id.clone());
            match oldest {
                Some(id) => entries.remove(&id),
                None => break,
            };
        }
        atomic_private_write(&self.path, encode(&entries).as_bytes())?;
        self.entries = entries;
        Ok(())
    }
}

fn report_unreadable(detail: &str) {
    demo_log::event(
        Kind::Sync,
        "LOCAL",
        "Profile store unreadable; starting empty",
        &[detail.to_owned()],
    );
}

/// Accept loop for the profile port. Runs on the directory's `LocalSet`;
/// callers stop it by aborting its task. An `accept()` error never ends it.
pub async fn serve(listener: TcpListener, store: Rc<RefCell<ProfileStore>>) -> Result<()> {
    let active = Rc::new(Cell::new(0usize));
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                super::listener_hiccup(&error).await;
                continue;
            }
        };
        let Some(permit) = ConnectionPermit::acquire(&active, MAX_PROFILE_CONNECTIONS) else {
            drop(stream);
            continue;
        };
        let store = Rc::clone(&store);
        tokio::task::spawn_local(async move {
            let _permit = permit;
            // Nothing on this port is an incident: a dropped connection is the
            // ordinary end of a request, not an event worth a red line.
            let _ = serve_connection(stream, store).await;
        });
    }
}

struct ConnectionPermit(Rc<Cell<usize>>);

impl ConnectionPermit {
    fn acquire(counter: &Rc<Cell<usize>>, limit: usize) -> Option<Self> {
        if counter.get() >= limit {
            return None;
        }
        counter.set(counter.get() + 1);
        Some(Self(Rc::clone(counter)))
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
}

async fn serve_connection(stream: TcpStream, store: Rc<RefCell<ProfileStore>>) -> Result<()> {
    let mut reader = BufReader::new(stream);
    for _ in 0..MAX_REQUESTS_PER_CONNECTION {
        let Some(line) = read_line(&mut reader).await? else {
            return Ok(());
        };
        if line.is_empty() {
            continue;
        }
        let reply = handle(&store, &line);
        write_line(reader.get_mut(), &reply).await?;
    }
    Ok(())
}

fn handle(store: &Rc<RefCell<ProfileStore>>, line: &str) -> String {
    let mut words = line.split_whitespace();
    match words.next().unwrap_or_default() {
        "PING" => "OK".to_owned(),
        "PROFILE_PUT" => match (words.next(), words.next()) {
            (Some(client_id), Some(name)) => put(store, client_id, name),
            _ => "ERR bad request".to_owned(),
        },
        "PROFILE_GET" => match words.next() {
            Some(client_id) => get(store, client_id),
            None => "ERR bad request".to_owned(),
        },
        _ => "ERR unknown command".to_owned(),
    }
}

fn put(store: &Rc<RefCell<ProfileStore>>, client_id: &str, name_b64: &str) -> String {
    if !is_client_id(client_id) {
        return "ERR bad client id".to_owned();
    }
    let decoded = decode_base64(name_b64).and_then(|bytes| String::from_utf8(bytes).ok());
    let Some(name) = decoded.as_deref().and_then(normalize_name) else {
        return "ERR bad name".to_owned();
    };
    if let Err(error) = store.borrow_mut().put(client_id, &name) {
        return format!("ERR {}", one_line(&error.to_string()));
    }
    demo_log::event(
        Kind::Membership,
        "LOCAL",
        format!(
            "Display name stored | client {} | name {name}",
            &client_id[..12]
        ),
        &[],
    );
    "OK".to_owned()
}

fn get(store: &Rc<RefCell<ProfileStore>>, client_id: &str) -> String {
    if !is_client_id(client_id) {
        return "ERR bad client id".to_owned();
    }
    match store.borrow().get(client_id) {
        Some(name) => format!("OK {}", encode_base64(name.as_bytes())),
        None => "ERR not found".to_owned(),
    }
}

fn is_client_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Display names are cosmetic, but they still reach other people's screens and
/// a demo terminal: bounded length and no control characters at all.
fn normalize_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let length = trimmed.chars().count();
    if length == 0 || length > MAX_NAME_CHARS || trimmed.chars().any(char::is_control) {
        return None;
    }
    Some(trimmed.to_owned())
}

async fn read_line(reader: &mut BufReader<TcpStream>) -> Result<Option<String>> {
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let available = tokio::time::timeout(PROFILE_IDLE_TIMEOUT, reader.fill_buf())
            .await
            .map_err(|_| Error::State("profile idle timeout"))??;
        if available.is_empty() {
            return Ok(None);
        }
        let (chunk, complete) = match available.iter().position(|byte| *byte == b'\n') {
            Some(index) => (&available[..index], true),
            None => (available, false),
        };
        let taken = chunk.len();
        if bytes.len().saturating_add(taken) > MAX_PROFILE_LINE {
            return Err(Error::InvalidInput("profile line is too long"));
        }
        bytes.extend_from_slice(chunk);
        reader.consume(taken + usize::from(complete));
        if complete {
            let text = String::from_utf8(bytes)
                .map_err(|_| Error::InvalidInput("profile line is not UTF-8"))?;
            return Ok(Some(text.trim().to_owned()));
        }
    }
}

async fn write_line(stream: &mut TcpStream, text: &str) -> Result<()> {
    let mut line = text.to_owned();
    line.push('\n');
    tokio::time::timeout(PROFILE_IDLE_TIMEOUT, stream.write_all(line.as_bytes()))
        .await
        .map_err(|_| Error::State("profile write timeout"))??;
    Ok(())
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// profiles.json
// ---------------------------------------------------------------------------

fn encode(entries: &BTreeMap<String, Profile>) -> String {
    let mut json = String::from("{");
    for (index, (client_id, profile)) in entries.iter().enumerate() {
        if index != 0 {
            json.push(',');
        }
        // Every stored key is validated 32-character lowercase hex, so only
        // the display name can ever need escaping.
        json.push_str(&format!(
            "\"{client_id}\":{{\"name\":\"{}\",\"updatedMs\":{}}}",
            escape(&profile.name),
            profile.updated_ms
        ));
    }
    json.push('}');
    json
}

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            _ => escaped.push(character),
        }
    }
    escaped
}

/// A deliberately small reader for the one shape this file ever has. Anything
/// else returns `None`, and the caller treats that as an empty store.
fn decode(bytes: &[u8]) -> Option<BTreeMap<String, Profile>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut cursor = Cursor::new(text);
    let mut entries = BTreeMap::new();
    cursor.expect('{')?;
    if !cursor.eat('}') {
        loop {
            let client_id = cursor.string()?;
            cursor.expect(':')?;
            cursor.expect('{')?;
            let mut name = None;
            let mut updated_ms = None;
            loop {
                match cursor.string()?.as_str() {
                    "name" => {
                        cursor.expect(':')?;
                        name = Some(cursor.string()?);
                    }
                    "updatedMs" => {
                        cursor.expect(':')?;
                        updated_ms = Some(cursor.number()?);
                    }
                    _ => return None,
                }
                if !cursor.eat(',') {
                    break;
                }
            }
            cursor.expect('}')?;
            if !is_client_id(&client_id) || entries.len() >= MAX_PROFILES {
                return None;
            }
            entries.insert(
                client_id,
                Profile {
                    name: normalize_name(&name?)?,
                    updated_ms: updated_ms?,
                },
            );
            if !cursor.eat(',') {
                break;
            }
        }
        cursor.expect('}')?;
    }
    cursor.end()?;
    Some(entries)
}

struct Cursor {
    chars: Vec<char>,
    index: usize,
}

impl Cursor {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            index: 0,
        }
    }

    fn skip_whitespace(&mut self) {
        while self
            .chars
            .get(self.index)
            .is_some_and(|character| character.is_ascii_whitespace())
        {
            self.index += 1;
        }
    }

    fn eat(&mut self, expected: char) -> bool {
        self.skip_whitespace();
        if self.chars.get(self.index) == Some(&expected) {
            self.index += 1;
            return true;
        }
        false
    }

    fn expect(&mut self, expected: char) -> Option<()> {
        self.eat(expected).then_some(())
    }

    fn string(&mut self) -> Option<String> {
        self.expect('"')?;
        let mut value = String::new();
        loop {
            let character = *self.chars.get(self.index)?;
            self.index += 1;
            match character {
                '"' => return Some(value),
                '\\' => {
                    let escaped = *self.chars.get(self.index)?;
                    self.index += 1;
                    value.push(match escaped {
                        '"' => '"',
                        '\\' => '\\',
                        '/' => '/',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        'b' => '\u{8}',
                        'f' => '\u{c}',
                        'u' => self.unicode()?,
                        _ => return None,
                    });
                }
                _ => value.push(character),
            }
        }
    }

    fn unicode(&mut self) -> Option<char> {
        let mut code = 0u32;
        for _ in 0..4 {
            let digit = self.chars.get(self.index)?.to_digit(16)?;
            self.index += 1;
            code = code * 16 + digit;
        }
        char::from_u32(code)
    }

    fn number(&mut self) -> Option<u64> {
        self.skip_whitespace();
        let start = self.index;
        while self
            .chars
            .get(self.index)
            .is_some_and(|character| character.is_ascii_digit())
        {
            self.index += 1;
        }
        if start == self.index {
            return None;
        }
        self.chars[start..self.index]
            .iter()
            .collect::<String>()
            .parse()
            .ok()
    }

    fn end(&mut self) -> Option<()> {
        self.skip_whitespace();
        (self.index == self.chars.len()).then_some(())
    }
}

// ---------------------------------------------------------------------------
// Standard Base64, padding required. Hand-rolled so no dependency is added.
// ---------------------------------------------------------------------------

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_base64(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut block = [0u8; 3];
        block[..chunk.len()].copy_from_slice(chunk);
        let bits = (u32::from(block[0]) << 16) | (u32::from(block[1]) << 8) | u32::from(block[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                output.push(char::from(
                    BASE64[((bits >> (18 - index * 6)) & 0x3f) as usize],
                ));
            } else {
                output.push('=');
            }
        }
    }
    output
}

fn decode_base64(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || !value.len().is_multiple_of(4) {
        return None;
    }
    let padding = value.bytes().rev().take_while(|byte| *byte == b'=').count();
    if padding > 2 {
        return None;
    }
    let digits = &value.as_bytes()[..value.len() - padding];
    let mut output = Vec::with_capacity(digits.len() * 3 / 4);
    let mut bits = 0u32;
    let mut available = 0u32;
    for &byte in digits {
        let digit = BASE64.iter().position(|candidate| *candidate == byte)? as u32;
        bits = (bits << 6) | digit;
        available += 6;
        if available >= 8 {
            available -= 8;
            output.push((bits >> available) as u8);
        }
    }
    // Non-canonical trailing bits are rejected, as the Base32 reader does.
    if available != 0 && bits & ((1u32 << available) - 1) != 0 {
        return None;
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        net::{Ipv4Addr, SocketAddr},
        sync::atomic::{AtomicU64, Ordering},
    };
    use tokio::io::AsyncReadExt;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temp_directory() -> Result<PathBuf> {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "qfs-profiles-test-{}-{sequence}-{}",
            std::process::id(),
            unix_millis()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    const ID: &str = "0123456789abcdef0123456789abcdef";
    const OTHER: &str = "ffffffffffffffffffffffffffffffff";

    async fn request(addr: SocketAddr, line: &str) -> Result<String> {
        let mut stream = TcpStream::connect(addr).await?;
        stream.write_all(format!("{line}\n").as_bytes()).await?;
        let mut reply = String::new();
        stream.read_to_string(&mut reply).await?;
        Ok(reply.trim_end().to_owned())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn line_protocol_round_trips_over_tcp() -> Result<()> {
        let directory = temp_directory()?;
        let local = tokio::task::LocalSet::new();
        let result = local
            .run_until(async {
                let store = Rc::new(RefCell::new(ProfileStore::open(
                    &directory.join("profiles.json"),
                )));
                let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
                let addr = listener.local_addr()?;
                let server = tokio::task::spawn_local(serve(listener, Rc::clone(&store)));

                assert_eq!(request(addr, "PING").await?, "OK");
                assert_eq!(
                    request(addr, &format!("PROFILE_GET {ID}")).await?,
                    "ERR not found"
                );
                assert_eq!(
                    request(addr, &format!("PROFILE_PUT {ID} QWFyeWFtYW4=")).await?,
                    "OK"
                );
                assert_eq!(
                    request(addr, &format!("PROFILE_GET {ID}")).await?,
                    "OK QWFyeWFtYW4="
                );
                assert_eq!(
                    request(addr, "PROFILE_GET 0123").await?,
                    "ERR bad client id"
                );
                assert_eq!(
                    request(addr, &format!("PROFILE_PUT {} QQ==", ID.to_uppercase())).await?,
                    "ERR bad client id"
                );
                assert_eq!(
                    request(addr, &format!("PROFILE_PUT {ID} ***")).await?,
                    "ERR bad name"
                );
                assert_eq!(
                    request(addr, &format!("PROFILE_PUT {ID} {}", encode_base64(b"   "))).await?,
                    "ERR bad name"
                );
                assert_eq!(
                    request(
                        addr,
                        &format!("PROFILE_PUT {ID} {}", encode_base64(&[0xff]))
                    )
                    .await?,
                    "ERR bad name"
                );
                assert_eq!(request(addr, "WHO_ARE_YOU").await?, "ERR unknown command");

                server.abort();
                assert_eq!(store.borrow().get(ID), Some("Aaryaman"));
                Ok::<(), Error>(())
            })
            .await;
        let _ = std::fs::remove_dir_all(&directory);
        result
    }

    #[test]
    fn profiles_survive_a_reopen_and_corruption_is_not_fatal() -> Result<()> {
        let directory = temp_directory()?;
        let path = directory.join("profiles.json");
        let result = (|| -> Result<()> {
            let mut store = ProfileStore::open(&path);
            store.put(ID, "Aaryaman \"A\" \\ Ops")?;
            store.put(OTHER, "Zoë")?;
            let reopened = ProfileStore::open(&path);
            assert_eq!(reopened.get(ID), Some("Aaryaman \"A\" \\ Ops"));
            assert_eq!(reopened.get(OTHER), Some("Zoë"));

            std::fs::write(&path, b"{ not json at all")?;
            assert_eq!(ProfileStore::open(&path).get(OTHER), None);
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&directory);
        result
    }

    #[test]
    fn base64_is_standard_and_requires_padding() {
        assert_eq!(encode_base64(b"Aaryaman"), "QWFyeWFtYW4=");
        assert_eq!(decode_base64("QWFyeWFtYW4=").unwrap(), b"Aaryaman");
        assert_eq!(encode_base64(b"A"), "QQ==");
        assert_eq!(decode_base64("QQ==").unwrap(), b"A");
        assert_eq!(encode_base64(b"AB"), "QUI=");
        assert_eq!(decode_base64("QUI=").unwrap(), b"AB");
        assert!(decode_base64("QQ").is_none());
        assert!(decode_base64("QR==").is_none());
        assert!(decode_base64("QWFy=").is_none());
        assert!(decode_base64("****").is_none());
    }

    #[test]
    fn names_are_bounded_and_free_of_control_characters() {
        assert_eq!(normalize_name("  Ada  ").as_deref(), Some("Ada"));
        assert_eq!(normalize_name("   "), None);
        assert_eq!(normalize_name("bad\nname"), None);
        assert!(normalize_name(&"a".repeat(MAX_NAME_CHARS)).is_some());
        assert_eq!(normalize_name(&"a".repeat(MAX_NAME_CHARS + 1)), None);
    }
}
