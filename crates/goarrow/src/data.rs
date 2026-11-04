//! Where GoArrow keeps its data in the plugin's storage, and the pieces of
//! loading it: the Atlas cache and its metadata
//! (`WarcryAtlasDataProvider.cs`), the indoor marks, the corrections file,
//! the download address check, and the load jobs the plugin runs a little
//! of every tick.

use serde::{Deserialize, Serialize};

use crate::database::{ATLAS_MAX_BYTES, AtlasError, LoadError, ParsedLocations};
use crate::loading::{AtlasJob, LocationsJob};

/// The last good Atlas download.
pub const CACHE_KEY: &str = "data/warcry-atlas.xml";
/// When it was downloaded, how many places it has, and from where.
pub const METADATA_KEY: &str = "data/warcry-atlas.metadata.json";
/// The player's indoor marks (`/go mark`).
pub const INDOOR_KEY: &str = "GoArrow/indoor-locations.xml";
/// The player's route corrections.
pub const CORRECTIONS_KEY: &str = "GoArrow/atlas-corrections.json";
/// The folder `/go file` reads from.
pub const FILE_FOLDER: &str = "GoArrow/";
/// The file `/go dungeon export|import` uses without a name.
pub const DEFAULT_SHARE_FILE: &str = "dungeon-traversals-share.json";
/// Darktorizo's GoArrow data, which the Options tab offers to fill in.
pub const SUGGESTED_DATA_URL: &str =
    "https://raw.githubusercontent.com/Darktorizo/GoArrow_Data_CoD/master/data_cod.xml";

/// The Atlas cache's metadata file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CacheMetadata {
    /// When, ISO 8601 with its offset.
    pub downloaded_at: String,
    /// How many `/atlas/location` records it has.
    pub location_count: usize,
    /// Where it came from.
    pub source_url: String,
}

/// GoArrow's download address check (`Uri.TryCreate(url.Trim(),
/// UriKind.Absolute)` with an `http` or `https` scheme): the address as it
/// is kept, its scheme and host in lower case and an empty path written
/// `/`, as .NET writes a `Uri`; none when it is not one.
#[must_use]
pub fn normalize_download_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let (scheme, rest) = trimmed.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    if rest.is_empty() || rest.chars().any(char::is_whitespace) {
        return None;
    }
    let split = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(split);
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host_name = host.split(':').next().unwrap_or("");
    if host_name.is_empty() {
        return None;
    }
    let tail = if tail.is_empty() || tail.starts_with(['?', '#']) {
        format!("/{tail}")
    } else {
        tail.to_owned()
    };
    Some(format!(
        "{scheme}://{}{tail}",
        authority.to_ascii_lowercase()
    ))
}

/// `/go file`'s rule for the name: relative, no `..`, not already under
/// `GoArrow/`, ending `.xml`. The storage key, or none.
#[must_use]
pub fn data_file_key(name: &str) -> Option<String> {
    let relative = name.replace('\\', "/");
    let refused = relative.starts_with('/')
        || relative.contains("..")
        || relative
            .get(..FILE_FOLDER.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(FILE_FOLDER))
        || !relative.to_ascii_lowercase().ends_with(".xml");
    (!refused).then(|| format!("{FILE_FOLDER}{relative}"))
}

/// `/go dungeon export|import`'s rule for the name: a file in the storage
/// folder ending `.json`.
#[must_use]
pub fn share_file_ok(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\'])
        && name != "."
        && name != ".."
        && name.to_ascii_lowercase().ends_with(".json")
}

/// The checks on a downloaded body before it is read (after an HTTP 200):
/// not empty, at most 16 MiB; its UTF-8 text.
///
/// # Errors
/// Why it was refused.
pub fn download_text(body: &[u8]) -> Result<String, AtlasError> {
    if body.is_empty() {
        return Err(AtlasError::Empty);
    }
    if body.len() > ATLAS_MAX_BYTES {
        return Err(AtlasError::TooLarge);
    }
    Ok(String::from_utf8_lossy(body).into_owned())
}

/// What a load job is for, and what is done with its result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadPurpose {
    /// The cached Atlas data, at start.
    StartupCache,
    /// A download (its text, cached once it reads, and its address).
    Download {
        /// The downloaded text.
        text: String,
        /// Where it came from.
        url: String,
    },
    /// The cached Atlas data after a failed download.
    CacheFallback,
    /// `/go file` (the name as typed, and its storage key).
    File {
        /// The name as typed.
        name: String,
        /// The storage key.
        key: String,
    },
}

/// A reading job.
#[derive(Debug)]
pub enum LoadJob {
    /// Atlas data.
    Atlas(AtlasJob),
    /// Any location file.
    Locations(LocationsJob),
}

/// A finished reading job.
#[derive(Debug)]
pub enum LoadResult {
    /// Atlas data and its record count.
    Atlas(Result<(ParsedLocations, usize), AtlasError>),
    /// A location file.
    Locations(Result<ParsedLocations, LoadError>),
}

impl LoadJob {
    /// At most `max_ops` units of work; the result once done.
    pub fn step(&mut self, max_ops: usize) -> Option<LoadResult> {
        match self {
            Self::Atlas(job) => job.step(max_ops).map(LoadResult::Atlas),
            Self::Locations(job) => job.step(max_ops).map(LoadResult::Locations),
        }
    }
}

/// A queued load.
#[derive(Debug)]
pub struct LoadTask {
    /// What it is for.
    pub purpose: LoadPurpose,
    /// The reading.
    pub job: LoadJob,
}
