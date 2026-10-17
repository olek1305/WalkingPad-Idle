//! Niezależny, wersjonowany zapis kampanii ewolucji.
//!
//! Ten moduł celowo nie zna starego `save.json`. Odczyt błędnego lub nowszego
//! pliku przełącza profil w stan tylko do odczytu, a zapis korzysta z osobnego
//! lease, tymczasowego pliku i kopii poprzedniego poprawnego profilu.

use super::catalog;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(target_os = "linux")]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SAVE_PATH: &str = "evolution-save.json";
pub const BACKUP_PATH: &str = "evolution-save.json.bak";
const LOCK_PATH: &str = "evolution-save.json.lock";
pub const SCHEMA_VERSION: u32 = 1;
pub const CONTENT_VERSION: u32 = 1;
pub const DEFAULT_SEED: u64 = 0xE701_2026_1009;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvolutionSave {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_content_version")]
    pub content_version: u32,
    #[serde(default = "default_seed")]
    pub seed: u64,
    #[serde(default)]
    pub treadmill_distance_km: f64,
    #[serde(default)]
    pub active_seconds: f64,
    #[serde(default)]
    pub knowledge: f64,
    #[serde(default)]
    pub earned_knowledge: f64,
    #[serde(default = "default_chapter_id")]
    pub chapter_id: String,
    #[serde(default = "default_habitat_id")]
    pub habitat_id: String,
    #[serde(default = "default_organism_id")]
    pub organism_id: String,
    #[serde(default)]
    pub unlocked_discovery_ids: Vec<String>,
    #[serde(default)]
    pub read_discovery_ids: Vec<String>,
    #[serde(default)]
    pub observation_accuracy_level: u8,
    #[serde(default)]
    pub observation_range_level: u8,
}

impl Default for EvolutionSave {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            content_version: CONTENT_VERSION,
            seed: DEFAULT_SEED,
            treadmill_distance_km: 0.0,
            active_seconds: 0.0,
            knowledge: 0.0,
            earned_knowledge: 0.0,
            chapter_id: catalog::FIRST_CHAPTER_ID.to_string(),
            habitat_id: catalog::FIRST_HABITAT_ID.to_string(),
            organism_id: catalog::FIRST_ORGANISM_ID.to_string(),
            unlocked_discovery_ids: Vec::new(),
            read_discovery_ids: Vec::new(),
            observation_accuracy_level: 0,
            observation_range_level: 0,
        }
    }
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

fn default_content_version() -> u32 {
    CONTENT_VERSION
}

fn default_seed() -> u64 {
    DEFAULT_SEED
}

fn default_chapter_id() -> String {
    catalog::FIRST_CHAPTER_ID.to_string()
}

fn default_habitat_id() -> String {
    catalog::FIRST_HABITAT_ID.to_string()
}

fn default_organism_id() -> String {
    catalog::FIRST_ORGANISM_ID.to_string()
}

impl EvolutionSave {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ValidationError::Future(format!(
                "schema_version={} nie jest obsługiwany",
                self.schema_version
            )));
        }
        if self.content_version != CONTENT_VERSION {
            if self.content_version > CONTENT_VERSION {
                return Err(ValidationError::Future(format!(
                    "content_version={} nie jest obsługiwany",
                    self.content_version
                )));
            }
            return Err(ValidationError::Corrupt(format!(
                "content_version={} jest nieprawidłowy",
                self.content_version
            )));
        }
        for (name, value) in [
            ("treadmill_distance_km", self.treadmill_distance_km),
            ("active_seconds", self.active_seconds),
            ("knowledge", self.knowledge),
            ("earned_knowledge", self.earned_knowledge),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(ValidationError::Corrupt(format!(
                    "pole {name} musi być skończoną liczbą nieujemną"
                )));
            }
        }
        validate_identifier("chapter_id", &self.chapter_id)?;
        validate_identifier("habitat_id", &self.habitat_id)?;
        validate_identifier("organism_id", &self.organism_id)?;
        if self.chapter_id != catalog::FIRST_CHAPTER_ID
            || self.habitat_id != catalog::FIRST_HABITAT_ID
            || self.organism_id != catalog::FIRST_ORGANISM_ID
        {
            return Err(ValidationError::Unsupported(
                "profil wskazuje na nieobsługiwany rozdział, habitat lub organizm".to_string(),
            ));
        }
        validate_id_list("unlocked_discovery_ids", &self.unlocked_discovery_ids)?;
        validate_id_list("read_discovery_ids", &self.read_discovery_ids)?;
        if let Some(id) = self.read_discovery_ids.iter().find(|id| {
            !self
                .unlocked_discovery_ids
                .iter()
                .any(|unlocked| unlocked == *id)
        }) {
            return Err(ValidationError::Corrupt(format!(
                "odkrycie {id:?} jest oznaczone jako przeczytane, ale nie jest odblokowane"
            )));
        }
        if let Some(id) = self.unlocked_discovery_ids.iter().find(|id| {
            catalog::discovery(id).is_some_and(|discovery| {
                self.treadmill_distance_km + f64::EPSILON < discovery.distance_km
            })
        }) {
            let distance = catalog::discovery(id)
                .map(|discovery| discovery.distance_km)
                .unwrap_or(0.0);
            return Err(ValidationError::Corrupt(format!(
                "odkrycie {id:?} wymaga dystansu co najmniej {distance} km"
            )));
        }
        if self.observation_accuracy_level
            > catalog::upgrade(catalog::UpgradeKind::Accuracy).max_level
            || self.observation_range_level
                > catalog::upgrade(catalog::UpgradeKind::Range).max_level
        {
            return Err(ValidationError::Corrupt(
                "poziom ulepszenia przekracza limit pierwszego wycinka".to_string(),
            ));
        }
        Ok(())
    }
}

fn validate_identifier(name: &str, value: &str) -> Result<(), ValidationError> {
    let valid = !value.is_empty()
        && value.len() <= 96
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        });
    if valid {
        Ok(())
    } else {
        Err(ValidationError::Corrupt(format!(
            "identyfikator {name} ma nieprawidłowy format"
        )))
    }
}

fn validate_id_list(name: &str, ids: &[String]) -> Result<(), ValidationError> {
    for id in ids {
        validate_identifier(name, id)?;
        if !catalog::is_known_discovery(id) {
            return Err(ValidationError::Unsupported(format!(
                "lista {name} zawiera nieobsługiwane ID {id:?}"
            )));
        }
    }
    for (index, id) in ids.iter().enumerate() {
        if ids[..index].iter().any(|previous| previous == id) {
            return Err(ValidationError::Corrupt(format!(
                "lista {name} zawiera powtórzone ID {id:?}"
            )));
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub enum ValidationError {
    Corrupt(String),
    Unsupported(String),
    Future(String),
}

impl ValidationError {
    fn message(&self) -> &str {
        match self {
            Self::Corrupt(message) | Self::Unsupported(message) | Self::Future(message) => message,
        }
    }
}

#[derive(Debug)]
enum ReadError {
    NotFound,
    Io(String),
    Corrupt(String),
    Unsupported(String),
    Future(String),
}

impl ReadError {
    fn description(&self) -> String {
        match self {
            Self::NotFound => "brak pliku".to_string(),
            Self::Io(message)
            | Self::Corrupt(message)
            | Self::Unsupported(message)
            | Self::Future(message) => message.clone(),
        }
    }
}

fn read_state_file(path: &Path) -> Result<EvolutionSave, ReadError> {
    let bytes = fs::read(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ReadError::NotFound
        } else {
            ReadError::Io(format!("nie można odczytać {}: {error}", path.display()))
        }
    })?;
    decode_state(&bytes, path)
}

fn decode_state(bytes: &[u8], path: &Path) -> Result<EvolutionSave, ReadError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| {
        ReadError::Corrupt(format!(
            "{} zawiera niepoprawny JSON: {error}",
            path.display()
        ))
    })?;
    let object = value.as_object().ok_or_else(|| {
        ReadError::Corrupt(format!("{} nie zawiera obiektu profilu", path.display()))
    })?;
    for required in [
        "schema_version",
        "content_version",
        "seed",
        "treadmill_distance_km",
        "active_seconds",
        "knowledge",
        "earned_knowledge",
        "chapter_id",
        "habitat_id",
        "organism_id",
    ] {
        if !object.contains_key(required) {
            return Err(ReadError::Corrupt(format!(
                "{} nie zawiera wymaganego pola {required:?}",
                path.display()
            )));
        }
    }
    let state: EvolutionSave = serde_json::from_value(value).map_err(|error| {
        ReadError::Corrupt(format!(
            "{} ma nieprawidłowy schemat: {error}",
            path.display()
        ))
    })?;
    match state.validate() {
        Ok(()) => Ok(state),
        Err(ValidationError::Corrupt(message)) => Err(ReadError::Corrupt(message)),
        Err(ValidationError::Unsupported(message)) => Err(ReadError::Unsupported(message)),
        Err(ValidationError::Future(message)) => Err(ReadError::Future(message)),
    }
}

pub struct OpenedProfile {
    pub state: EvolutionSave,
    pub persistence: Persistence,
}

pub struct Persistence {
    read_only: bool,
    status: String,
    lease: Option<Lease>,
    new_profile: bool,
}

enum LoadedProfile {
    Primary(EvolutionSave),
    Blocked {
        state: EvolutionSave,
        status: String,
    },
    Recovered {
        state: EvolutionSave,
        status: String,
    },
    New,
    Invalid(String),
}

fn load_profile() -> LoadedProfile {
    match read_state_file(Path::new(SAVE_PATH)) {
        Ok(state) => match read_state_file(Path::new(BACKUP_PATH)) {
            Ok(_) | Err(ReadError::NotFound) => LoadedProfile::Primary(state),
            Err(error) => LoadedProfile::Blocked {
                state,
                status: format!(
                    "Profil główny jest poprawny, ale istniejąca kopia .bak jest nieprawidłowa ({}); zapis zablokowany, a kopia pozostaje nietknięta.",
                    error.description()
                ),
            },
        },
        Err(ReadError::NotFound) => match read_state_file(Path::new(BACKUP_PATH)) {
            Ok(state) => LoadedProfile::Recovered {
                state,
                status: "Odzyskano poprawną kopię .bak; profil jest tylko do odczytu, a oryginał pozostaje nietknięty."
                    .to_string(),
            },
            Err(ReadError::NotFound) => LoadedProfile::New,
            Err(error) => LoadedProfile::Invalid(format!(
                "Brak profilu głównego i nie można użyć kopii .bak: {}",
                error.description()
            )),
        },
        Err(primary_error) => match read_state_file(Path::new(BACKUP_PATH)) {
            Ok(state) => LoadedProfile::Recovered {
                state,
                status: format!(
                    "Profil główny jest niedostępny ({}). Użyto poprawnej kopii .bak tylko do odczytu; zapis zablokowany.",
                    primary_error.description()
                ),
            },
            Err(backup_error) => LoadedProfile::Invalid(format!(
                "Nie można bezpiecznie wczytać profilu (główny: {}; kopia .bak: {}). Zapis zablokowany.",
                primary_error.description(),
                backup_error.description()
            )),
        },
    }
}

impl Persistence {
    pub fn open() -> OpenedProfile {
        // A writable session must own the lease before it reads any state.
        // Otherwise another writer could publish newer data between our read
        // and a later lease acquisition.
        match Lease::acquire() {
            Ok(lease) => match load_profile() {
                LoadedProfile::Primary(state) => Self::open_writable(state, false, lease),
                LoadedProfile::New => Self::open_writable(EvolutionSave::default(), true, lease),
                LoadedProfile::Blocked { state, status } => {
                    drop(lease);
                    Self::open_read_only(state, status, false)
                }
                LoadedProfile::Recovered { state, status } => {
                    drop(lease);
                    Self::open_read_only(state, status, false)
                }
                LoadedProfile::Invalid(status) => {
                    drop(lease);
                    Self::open_read_only(EvolutionSave::default(), status, false)
                }
            },
            Err(lock_error) => {
                // This is deliberately a permanently read-only view. It may
                // be stale, so save() never attempts to acquire a lease later.
                let lock_status = format!("Zapis zablokowany: {lock_error}");
                match load_profile() {
                    LoadedProfile::Primary(state) => Self::open_read_only(
                        state,
                        format!("{lock_status}. Wczytano profil tylko do odczytu."),
                        false,
                    ),
                    LoadedProfile::Blocked { state, status } => {
                        Self::open_read_only(state, format!("{lock_status}. {status}"), false)
                    }
                    LoadedProfile::Recovered { state, status } => {
                        Self::open_read_only(state, format!("{lock_status}. {status}"), false)
                    }
                    LoadedProfile::New => Self::open_read_only(
                        EvolutionSave::default(),
                        format!("{lock_status}. Nie utworzono nowego profilu."),
                        false,
                    ),
                    LoadedProfile::Invalid(status) => Self::open_read_only(
                        EvolutionSave::default(),
                        format!("{lock_status}. {status}"),
                        false,
                    ),
                }
            }
        }
    }

    fn open_writable(state: EvolutionSave, new_profile: bool, lease: Lease) -> OpenedProfile {
        OpenedProfile {
            state,
            persistence: Self {
                read_only: false,
                status: if new_profile {
                    "Nowy profil zapisywalny; pierwszy zapis zostanie wykonany natychmiast."
                } else {
                    "Profil wczytany; zapis chroni blokada pojedynczego właściciela."
                }
                .to_string(),
                lease: Some(lease),
                new_profile,
            },
        }
    }

    fn open_read_only(
        state: EvolutionSave,
        status: impl Into<String>,
        new_profile: bool,
    ) -> OpenedProfile {
        OpenedProfile {
            state,
            persistence: Self {
                read_only: true,
                status: status.into(),
                lease: None,
                new_profile,
            },
        }
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn is_new_profile(&self) -> bool {
        self.new_profile
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn save(&mut self, state: &EvolutionSave) -> Result<(), String> {
        if self.read_only {
            return Err(self.status.clone());
        }
        let lease_error = match self.lease.as_ref() {
            Some(lease) => lease.verify().err(),
            None => Some("brak własnej blokady zapisu".to_string()),
        };
        if let Some(error) = lease_error {
            self.read_only = true;
            self.lease.take();
            self.status =
                format!("Zapis zablokowany: blokada zmieniła się przed zapisem ({error}).");
            return Err(self.status.clone());
        }
        state.validate().map_err(|error| {
            let message = format!("Nie zapisano profilu: {}", error.message());
            self.status = message.clone();
            message
        })?;
        let bytes = serde_json::to_vec_pretty(state).map_err(|error| {
            let message = format!("Nie zapisano profilu: serializacja nie powiodła się: {error}");
            self.status = message.clone();
            message
        })?;
        let previous = match fs::read(SAVE_PATH) {
            Ok(bytes) => {
                if let Err(error) = decode_state(&bytes, Path::new(SAVE_PATH)) {
                    let message = format!(
                        "Nie zapisano profilu: bieżący plik zmienił się i jest niepoprawny ({})",
                        error.description()
                    );
                    self.status = message.clone();
                    return Err(message);
                }
                Some(bytes)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                let message =
                    format!("Nie zapisano profilu: nie można sprawdzić pliku głównego: {error}");
                self.status = message.clone();
                return Err(message);
            }
        };

        match read_state_file(Path::new(BACKUP_PATH)) {
            Ok(_) | Err(ReadError::NotFound) => {}
            Err(error) => {
                self.status = format!(
                    "Nie zapisano profilu: istniejąca kopia .bak jest nieprawidłowa ({}); nie zostanie zastąpiona.",
                    error.description()
                );
                return Err(self.status.clone());
            }
        }

        let token = unique_token();
        if let Some(previous) = previous.as_deref() {
            if let Err(message) = rotate_backup(previous, &token) {
                self.status = format!(
                    "Nie zapisano profilu: nie udało się trwale przygotować kopii bezpieczeństwa: {message}"
                );
                return Err(self.status.clone());
            }
        }
        if let Err(message) = atomic_write(Path::new(SAVE_PATH), &bytes, &token) {
            self.status = format!("Nie zapisano profilu: {message}");
            return Err(self.status.clone());
        }
        self.new_profile = false;
        self.status = format!("Zapisano profil ({SAVE_PATH}).");
        Ok(())
    }
}

struct Lease {
    path: PathBuf,
    contents: String,
}

impl Lease {
    fn acquire() -> Result<Self, String> {
        let contents = format!("pid={}\ntoken={}\n", process::id(), unique_token());
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(LOCK_PATH)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let detail = fs::read_to_string(LOCK_PATH)
                    .unwrap_or_else(|_| "nie można odczytać zawartości blokady".to_string());
                return Err(format!(
                    "istnieje evolution-save.json.lock ({}) — zweryfikuj proces właściciela przed usunięciem blokady",
                    detail.trim()
                ));
            }
            Err(error) => return Err(format!("nie można utworzyć blokady zapisu: {error}")),
        };
        if let Err(error) = file
            .write_all(contents.as_bytes())
            .and_then(|_| file.sync_all())
        {
            if fs::read_to_string(LOCK_PATH)
                .ok()
                .is_some_and(|current| current == contents)
            {
                let _ = fs::remove_file(LOCK_PATH);
            }
            return Err(format!("nie można zapisać blokady: {error}"));
        }
        Ok(Self {
            path: PathBuf::from(LOCK_PATH),
            contents,
        })
    }

    fn verify(&self) -> Result<(), String> {
        match fs::read_to_string(&self.path) {
            Ok(contents) if contents == self.contents => Ok(()),
            Ok(_) => Err("zawartość pliku blokady jest inna".to_string()),
            Err(error) => Err(format!("nie można odczytać pliku blokady: {error}")),
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.verify().is_ok() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

static TOKEN_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_token() -> String {
    let counter = TOKEN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}-{counter}", process::id())
}

fn temporary_path(target: &Path, token: &str, label: &str) -> PathBuf {
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("evolution-save");
    parent.join(format!(".{name}.{label}.{token}.tmp"))
}

fn write_temp(target: &Path, bytes: &[u8], token: &str, label: &str) -> Result<PathBuf, String> {
    let temp = temporary_path(target, token, label);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|error| format!("nie można utworzyć {}: {error}", temp.display()))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temp);
        return Err(format!("nie można zapisać {}: {error}", temp.display()));
    }
    Ok(temp)
}

fn atomic_write(target: &Path, bytes: &[u8], token: &str) -> Result<(), String> {
    let temp = write_temp(target, bytes, token, "primary")?;
    if let Err(error) = fs::rename(&temp, target) {
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "nie można opublikować {}: {error}",
            target.display()
        ));
    }
    sync_parent(target).map_err(|error| {
        format!(
            "opublikowano {}, ale nie można zsynchronizować katalogu nadrzędnego: {error}",
            target.display()
        )
    })?;
    Ok(())
}

fn rotate_backup(previous: &[u8], token: &str) -> Result<(), String> {
    let backup = Path::new(BACKUP_PATH);
    let temp = write_temp(backup, previous, token, "backup")?;
    if let Err(error) = fs::rename(&temp, backup) {
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "nie można opublikować kopii {}: {error}",
            backup.display()
        ));
    }
    sync_parent(backup).map_err(|error| {
        format!(
            "opublikowano kopię {}, ale nie można zsynchronizować katalogu nadrzędnego: {error}",
            backup.display()
        )
    })?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn sync_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let directory = File::open(parent)
        .map_err(|error| format!("nie można otworzyć katalogu {}: {error}", parent.display()))?;
    directory.sync_all().map_err(|error| {
        format!(
            "nie można zsynchronizować katalogu {}: {error}",
            parent.display()
        )
    })
}

#[cfg(not(target_os = "linux"))]
fn sync_parent(_path: &Path) -> Result<(), String> {
    Ok(())
}
