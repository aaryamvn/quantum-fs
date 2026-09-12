//! Signed join-code directory storage. This module stores no member lists, pair
//! keys, or file data. Advertised addresses are numeric `SocketAddr` values;
//! hostnames and DNS are intentionally outside the v1 protocol.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};

use rand::TryRng;
use tokio::{
    net::{TcpListener, TcpStream},
    time::{timeout, Duration},
};

use crate::{
    crypto::{
        sign::{PureMlDsa, RustCryptoPureMlDsa, DIRECTORY_CONTEXT},
        wrap::validate_public_key,
    },
    encoding,
    ids::PeerId,
    keystore::KeyStore,
    net::{JoinCode, VaultId},
    Error, Result,
};

use super::frame::{read_frame, write_frame, Frame};

const MAX_AD_AGE_SECS: u64 = 7 * 24 * 60 * 60;
const MAX_FUTURE_SKEW_SECS: u64 = 120;
const MAX_ADS_PER_PEER: usize = 32;
const MAX_TOTAL_ADS: usize = 10_000;
const MAX_STATE_BYTES: u64 = 128 * 1024 * 1024;
const DIRECTORY_DEADLINE: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: usize = 128;
const DIR_LOOKUP_KIND: u8 = 5;
const DIR_AD_KIND: u8 = 6;
const DIR_PUT_KIND: u8 = 7;
const DIR_FORGET_KIND: u8 = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryAd {
    pub peer_id: PeerId,
    pub vault_id: VaultId,
    pub addr: SocketAddr,
    pub ek: Vec<u8>,
    pub vk: Vec<u8>,
    pub issued_at: u64,
    pub signature: Vec<u8>,
}

impl DirectoryAd {
    pub fn sign(
        keys: &KeyStore,
        vault_id: VaultId,
        addr: SocketAddr,
        issued_at: u64,
    ) -> Result<Self> {
        require_advertisable(addr)?;
        let identity = keys.identity()?;
        let mut ad = Self {
            peer_id: identity.peer_id,
            vault_id,
            addr,
            ek: identity.ek,
            vk: identity.vk,
            issued_at,
            signature: Vec::new(),
        };
        ad.signature = RustCryptoPureMlDsa.sign(
            &keys.signing_key()?,
            DIRECTORY_CONTEXT,
            &encoding::directory_ad_m(&ad)?,
        )?;
        Ok(ad)
    }

    pub fn verify(&self, now: u64) -> Result<()> {
        require_current(self.issued_at, now)?;
        self.verify_signature()
    }

    fn verify_signature(&self) -> Result<()> {
        require_advertisable(self.addr)?;
        validate_public_key(&self.ek)?;
        if self.peer_id != encoding::peer_id(&self.vk) {
            return Err(Error::AuthenticationFailed);
        }
        RustCryptoPureMlDsa.verify(
            &self.vk,
            DIRECTORY_CONTEXT,
            &encoding::directory_ad_m(self)?,
            &self.signature,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirForget {
    pub peer_id: PeerId,
    pub vault_id: VaultId,
    pub join_code: JoinCode,
    pub issued_at: u64,
    pub signature: Vec<u8>,
}

impl DirForget {
    pub fn sign(
        keys: &KeyStore,
        vault_id: VaultId,
        join_code: JoinCode,
        issued_at: u64,
    ) -> Result<Self> {
        let mut request = Self {
            peer_id: keys.peer_id()?,
            vault_id,
            join_code,
            issued_at,
            signature: Vec::new(),
        };
        request.signature = RustCryptoPureMlDsa.sign(
            &keys.signing_key()?,
            DIRECTORY_CONTEXT,
            &encoding::dir_forget_m(&request),
        )?;
        Ok(request)
    }

    pub fn verify(&self, verification_key: &[u8], now: u64) -> Result<()> {
        require_current(self.issued_at, now)?;
        if self.peer_id != encoding::peer_id(verification_key) {
            return Err(Error::AuthenticationFailed);
        }
        RustCryptoPureMlDsa.verify(
            verification_key,
            DIRECTORY_CONTEXT,
            &encoding::dir_forget_m(self),
            &self.signature,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectoryWatermark {
    pub(crate) peer_id: PeerId,
    pub(crate) vault_id: VaultId,
    pub(crate) issued_at: u64,
    pub(crate) vk: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectoryState {
    pub(crate) ads: Vec<(JoinCode, DirectoryAd)>,
    pub(crate) watermarks: Vec<DirectoryWatermark>,
}

pub struct DirectoryStore {
    path: PathBuf,
    ads: BTreeMap<JoinCode, DirectoryAd>,
    watermarks: BTreeMap<(PeerId, VaultId), DirectoryWatermark>,
    _lock: File,
    failed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct DirectoryClient {
    addr: SocketAddr,
}

impl DirectoryClient {
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }

    pub async fn put(&self, code: JoinCode, ad: &DirectoryAd) -> Result<()> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&code.0);
        payload.extend_from_slice(&encoding::encode_directory_ad(ad)?);
        let response = self.exchange(Frame::new(DIR_PUT_KIND, payload)?).await?;
        require_empty_ack(response, DIR_PUT_KIND)
    }

    pub async fn lookup(&self, code: JoinCode) -> Result<Option<DirectoryAd>> {
        let request = Frame::new(DIR_LOOKUP_KIND, code.0.to_vec())?;
        let response = match self.exchange(request).await {
            Ok(response) => response,
            Err(Error::Io(error)) if error.kind() == ErrorKind::UnexpectedEof => return Ok(None),
            Err(error) => return Err(error),
        };
        if response.kind != DIR_AD_KIND {
            return Err(Error::InvalidInput("unexpected directory response type"));
        }
        Ok(Some(encoding::decode_directory_ad(&response.payload)?))
    }

    pub async fn forget(&self, request: &DirForget) -> Result<()> {
        let payload = encoding::encode_dir_forget(request)?;
        let response = self.exchange(Frame::new(DIR_FORGET_KIND, payload)?).await?;
        require_empty_ack(response, DIR_FORGET_KIND)
    }

    async fn exchange(&self, request: Frame) -> Result<Frame> {
        timeout(DIRECTORY_DEADLINE, async {
            let mut stream = TcpStream::connect(self.addr).await?;
            write_frame(&mut stream, &request).await?;
            read_frame(&mut stream).await
        })
        .await
        .map_err(|_| Error::State("directory request deadline exceeded"))?
    }
}

/// Serve one directory request per TCP connection. This must run inside the
/// daemon's current-thread `LocalSet`; callers stop it by aborting its task.
pub async fn serve(listener: TcpListener, store: Rc<RefCell<DirectoryStore>>) -> Result<()> {
    let active = Rc::new(Cell::new(0usize));
    loop {
        let (stream, _) = listener.accept().await?;
        if active.get() >= MAX_CONNECTIONS {
            drop(stream);
            continue;
        }
        active.set(active.get() + 1);
        let permit = ConnectionPermit {
            active: Rc::clone(&active),
        };
        let store = Rc::clone(&store);
        tokio::task::spawn_local(async move {
            let _permit = permit;
            let _ = timeout(DIRECTORY_DEADLINE, handle_connection(stream, store)).await;
        });
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    store: Rc<RefCell<DirectoryStore>>,
) -> Result<()> {
    let request = read_frame(&mut stream).await?;
    let now = unix_time()?;
    let response = match request.kind {
        DIR_LOOKUP_KIND => {
            let code = decode_join_code(&request.payload)?;
            let ad = store
                .borrow_mut()
                .lookup(&code, now)?
                .ok_or(Error::InvalidInput("directory join code not found"))?;
            Frame::new(DIR_AD_KIND, encoding::encode_directory_ad(&ad)?)?
        }
        DIR_PUT_KIND => {
            let (code, ad) = decode_put(&request.payload)?;
            store.borrow_mut().put(code, ad, now)?;
            Frame::new(DIR_PUT_KIND, Vec::new())?
        }
        DIR_FORGET_KIND => {
            let request = encoding::decode_dir_forget(&request.payload)?;
            store.borrow_mut().forget(request, now)?;
            Frame::new(DIR_FORGET_KIND, Vec::new())?
        }
        _ => return Err(Error::InvalidInput("unsupported directory request type")),
    };
    write_frame(&mut stream, &response).await
}

struct ConnectionPermit {
    active: Rc<Cell<usize>>,
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.active.set(self.active.get().saturating_sub(1));
    }
}

fn decode_join_code(payload: &[u8]) -> Result<JoinCode> {
    let bytes = <[u8; 16]>::try_from(payload)
        .map_err(|_| Error::InvalidInput("directory lookup requires a 16-byte join code"))?;
    Ok(JoinCode(bytes))
}

fn decode_put(payload: &[u8]) -> Result<(JoinCode, DirectoryAd)> {
    let (code, ad) = payload
        .split_at_checked(16)
        .ok_or(Error::InvalidInput("directory put omits join code"))?;
    Ok((decode_join_code(code)?, encoding::decode_directory_ad(ad)?))
}

fn require_empty_ack(response: Frame, expected_kind: u8) -> Result<()> {
    if response.kind != expected_kind || !response.payload.is_empty() {
        return Err(Error::InvalidInput("invalid directory acknowledgement"));
    }
    Ok(())
}

impl DirectoryStore {
    pub fn open(path: &Path) -> Result<Self> {
        ensure_parent(path)?;
        let lock_path = sibling(path, ".lock");
        let lock = private_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lock.try_lock()
            .map_err(|_| Error::State("directory store is already open"))?;

        let state = match read_private(path)? {
            Some(bytes) if !bytes.is_empty() => encoding::decode_directory_state(&bytes)?,
            _ => DirectoryState {
                ads: Vec::new(),
                watermarks: Vec::new(),
            },
        };
        let mut ads = BTreeMap::new();
        for (code, ad) in state.ads {
            ad.verify_signature()?;
            if ads.insert(code, ad).is_some() {
                return Err(Error::InvalidInput("duplicate directory join code"));
            }
        }
        let mut watermarks = BTreeMap::new();
        for watermark in state.watermarks {
            if watermark.peer_id != encoding::peer_id(&watermark.vk) {
                return Err(Error::AuthenticationFailed);
            }
            let key = (watermark.peer_id, watermark.vault_id);
            if watermarks.insert(key, watermark).is_some() {
                return Err(Error::InvalidInput("duplicate directory watermark"));
            }
        }
        if ads.len() > MAX_TOTAL_ADS || watermarks.len() > MAX_TOTAL_ADS {
            return Err(Error::InvalidInput("directory store exceeds entry cap"));
        }
        require_peer_caps(&ads)?;
        for ad in ads.values() {
            let watermark = watermarks
                .get(&(ad.peer_id, ad.vault_id))
                .ok_or(Error::InvalidInput("directory ad has no watermark"))?;
            if watermark.issued_at < ad.issued_at || watermark.vk != ad.vk {
                return Err(Error::InvalidInput("directory ad watermark mismatch"));
            }
        }
        Ok(Self {
            path: path.to_owned(),
            ads,
            watermarks,
            _lock: lock,
            failed: false,
        })
    }

    pub fn put(&mut self, code: JoinCode, ad: DirectoryAd, now: u64) -> Result<()> {
        self.require_healthy()?;
        ad.verify(now)?;
        let (mut ads, mut watermarks) = self.pruned(now);
        if let Some(existing) = ads.get(&code) {
            if existing.peer_id != ad.peer_id {
                return Err(Error::AuthenticationFailed);
            }
        }
        let pair = (ad.peer_id, ad.vault_id);
        if watermarks
            .get(&pair)
            .is_some_and(|watermark| ad.issued_at <= watermark.issued_at)
        {
            return Err(Error::ReplayRejected);
        }
        if !watermarks.contains_key(&pair) && watermarks.len() >= MAX_TOTAL_ADS {
            return Err(Error::InvalidInput("directory watermark cap reached"));
        }
        let replaces_same_peer = ads
            .get(&code)
            .is_some_and(|existing| existing.peer_id == ad.peer_id);
        let peer_count = ads
            .values()
            .filter(|existing| existing.peer_id == ad.peer_id)
            .count();
        if !replaces_same_peer && peer_count >= MAX_ADS_PER_PEER {
            return Err(Error::InvalidInput("directory peer ad cap reached"));
        }
        if !ads.contains_key(&code) && ads.len() >= MAX_TOTAL_ADS {
            return Err(Error::InvalidInput("directory ad cap reached"));
        }

        watermarks.insert(
            pair,
            DirectoryWatermark {
                peer_id: ad.peer_id,
                vault_id: ad.vault_id,
                issued_at: ad.issued_at,
                vk: ad.vk.clone(),
            },
        );
        ads.insert(code, ad);
        self.commit(ads, watermarks)
    }

    pub fn lookup(&mut self, code: &JoinCode, now: u64) -> Result<Option<DirectoryAd>> {
        self.require_healthy()?;
        let (ads, watermarks) = self.pruned(now);
        let changed = ads.len() != self.ads.len() || watermarks.len() != self.watermarks.len();
        if changed {
            self.commit(ads, watermarks)?;
        }
        Ok(self.ads.get(code).cloned())
    }

    pub fn forget(&mut self, request: DirForget, now: u64) -> Result<()> {
        self.require_healthy()?;
        let (mut ads, mut watermarks) = self.pruned(now);
        if let Some(existing) = ads.get(&request.join_code) {
            if existing.peer_id != request.peer_id || existing.vault_id != request.vault_id {
                return Err(Error::AuthenticationFailed);
            }
        }
        let pair = (request.peer_id, request.vault_id);
        let watermark = watermarks.get(&pair).ok_or(Error::AuthenticationFailed)?;
        request.verify(&watermark.vk, now)?;
        if request.issued_at <= watermark.issued_at {
            return Err(Error::ReplayRejected);
        }
        let vk = watermark.vk.clone();
        ads.remove(&request.join_code);
        watermarks.insert(
            pair,
            DirectoryWatermark {
                peer_id: request.peer_id,
                vault_id: request.vault_id,
                issued_at: request.issued_at,
                vk,
            },
        );
        self.commit(ads, watermarks)
    }

    fn pruned(
        &self,
        now: u64,
    ) -> (
        BTreeMap<JoinCode, DirectoryAd>,
        BTreeMap<(PeerId, VaultId), DirectoryWatermark>,
    ) {
        let mut ads = self.ads.clone();
        ads.retain(|_, ad| !is_expired(ad.issued_at, now));
        let mut watermarks = self.watermarks.clone();
        watermarks.retain(|_, watermark| !is_expired(watermark.issued_at, now));
        (ads, watermarks)
    }

    fn commit(
        &mut self,
        ads: BTreeMap<JoinCode, DirectoryAd>,
        watermarks: BTreeMap<(PeerId, VaultId), DirectoryWatermark>,
    ) -> Result<()> {
        let state = DirectoryState {
            ads: ads.iter().map(|(code, ad)| (*code, ad.clone())).collect(),
            watermarks: watermarks.values().cloned().collect(),
        };
        let bytes = encoding::encode_directory_state(&state)?;
        if let Err(error) = atomic_private_write(&self.path, &bytes) {
            // Rename may already have succeeded before a later directory fsync
            // failed. Stop serving rather than diverging from uncertain disk state.
            self.failed = true;
            return Err(error);
        }
        self.ads = ads;
        self.watermarks = watermarks;
        Ok(())
    }

    fn require_healthy(&self) -> Result<()> {
        if self.failed {
            return Err(Error::State(
                "directory persistence failed; reopen required",
            ));
        }
        Ok(())
    }
}

fn require_current(issued_at: u64, now: u64) -> Result<()> {
    if issued_at > now.saturating_add(MAX_FUTURE_SKEW_SECS) {
        return Err(Error::InvalidInput(
            "directory record is too far in the future",
        ));
    }
    if is_expired(issued_at, now) {
        return Err(Error::InvalidInput("directory record is expired"));
    }
    Ok(())
}

fn unix_time() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| Error::State("system clock is before Unix epoch"))
}

fn is_expired(issued_at: u64, now: u64) -> bool {
    now.saturating_sub(issued_at) > MAX_AD_AGE_SECS
}

fn require_advertisable(addr: SocketAddr) -> Result<()> {
    if addr.port() == 0 || addr.ip().is_unspecified() {
        return Err(Error::InvalidInput("directory address is not connectable"));
    }
    Ok(())
}

fn require_peer_caps(ads: &BTreeMap<JoinCode, DirectoryAd>) -> Result<()> {
    let mut counts = BTreeMap::<PeerId, usize>::new();
    for ad in ads.values() {
        let count = counts.entry(ad.peer_id).or_default();
        *count += 1;
        if *count > MAX_ADS_PER_PEER {
            return Err(Error::InvalidInput("directory peer ad cap exceeded"));
        }
    }
    Ok(())
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn read_private(path: &Path) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() {
        return Err(Error::InvalidInput(
            "directory store path must be a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(Error::InvalidInput(
                "directory store permissions must be owner-only (0600)",
            ));
        }
    }
    if metadata.len() > MAX_STATE_BYTES {
        return Err(Error::InvalidInput("directory store is too large"));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?
        .take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(Error::InvalidInput("directory store is too large"));
    }
    Ok(Some(bytes))
}

fn atomic_private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(Error::InvalidInput("directory store is too large"));
    }
    let mut suffix = [0; 8];
    rand::rngs::SysRng
        .try_fill_bytes(&mut suffix)
        .map_err(|_| Error::State("OS entropy unavailable"))?;
    let temporary = sibling(
        path,
        &format!(".tmp-{}-{}", std::process::id(), u64::from_be_bytes(suffix)),
    );
    let result = (|| -> Result<()> {
        let mut file = private_options()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
