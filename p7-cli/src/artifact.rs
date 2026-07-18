use crate::project::DependencyManifest;
use directories::ProjectDirs;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tar::EntryType;
use url::Url;

const INDEX_VERSION: u32 = 1;
const LOCK_VERSION: u32 = 2;
const CACHE_MARKER: &str = ".p7-artifact";
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactIndex {
    version: u32,
    package: IndexPackage,
    #[serde(default)]
    dependencies: BTreeMap<String, DependencyManifest>,
    targets: BTreeMap<String, IndexTarget>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexPackage {
    name: String,
    version: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexTarget {
    url: String,
    sha256: String,
    format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Lockfile {
    pub version: u32,
    pub package: Vec<LockedPackage>,
}

impl Lockfile {
    pub(crate) fn new(mut package: Vec<LockedPackage>) -> Self {
        package.sort_by(|left, right| left.name.cmp(&right.name));
        Self {
            version: LOCK_VERSION,
            package,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LockedPackage {
    pub name: String,
    pub version: String,
    pub source: LockedSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, DependencyManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum LockedSource {
    Path {
        path: String,
    },
    Artifact {
        index: String,
        index_sha256: String,
        targets: BTreeMap<String, LockedTarget>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LockedTarget {
    pub url: String,
    pub sha256: String,
    pub format: String,
}

#[derive(Debug, Deserialize)]
struct V1Lockfile {
    version: u32,
    #[serde(default)]
    packages: Vec<V1LockedPackage>,
}

#[derive(Debug, Deserialize)]
struct V1LockedPackage {
    name: String,
    version: String,
    source: String,
    checksum: String,
    #[serde(default)]
    dependencies: Vec<String>,
}

#[derive(Debug)]
enum ExistingLock {
    None,
    V1,
    V2(HashMap<String, LockedPackage>),
}

#[derive(Debug)]
pub(crate) struct ResolvedArtifact {
    pub root: PathBuf,
    pub version: String,
    pub dependencies: BTreeMap<String, DependencyManifest>,
    pub source: LockedSource,
}

pub(crate) struct ArtifactResolver {
    cache: Result<Cache, String>,
    target: Result<String, String>,
    lock: ExistingLock,
}

impl ArtifactResolver {
    pub(crate) fn for_project(project_root: &Path) -> Result<Self, String> {
        Ok(Self {
            cache: default_cache_dir().map(Cache::new),
            target: host_target(),
            lock: read_lockfile(project_root)?,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        project_root: &Path,
        cache_dir: PathBuf,
        target: &str,
    ) -> Result<Self, String> {
        Ok(Self {
            cache: Ok(Cache::new(cache_dir)),
            target: Ok(target.to_string()),
            lock: read_lockfile(project_root)?,
        })
    }

    pub(crate) fn resolve(
        &self,
        expected_name: &str,
        index_value: &str,
    ) -> Result<ResolvedArtifact, String> {
        let index_url = parse_source_url(index_value, "artifact index")?;
        if let Some(locked) = self.locked_package(expected_name, index_url.as_str())? {
            return self.resolve_locked(locked);
        }
        self.resolve_unlocked(expected_name, &index_url)
    }

    fn cache(&self) -> Result<&Cache, String> {
        self.cache.as_ref().map_err(Clone::clone)
    }

    fn locked_package(
        &self,
        expected_name: &str,
        index_url: &str,
    ) -> Result<Option<&LockedPackage>, String> {
        match &self.lock {
            ExistingLock::None => Ok(None),
            ExistingLock::V1 => Err(format!(
                "p7.lock version 1 cannot lock artifact dependency '{expected_name}'; remove it to generate lockfile version 2"
            )),
            ExistingLock::V2(packages) => {
                let Some(package) = packages.get(expected_name) else {
                    return Ok(None);
                };
                match &package.source {
                    LockedSource::Artifact { index, .. } if index == index_url => Ok(Some(package)),
                    LockedSource::Artifact { index, .. } => Err(format!(
                        "Locked package '{expected_name}' uses artifact index '{index}', but p7.toml specifies '{index_url}'"
                    )),
                    LockedSource::Path { .. } => Err(format!(
                        "Locked package '{expected_name}' is a path package, but p7.toml specifies an artifact index"
                    )),
                }
            }
        }
    }

    fn resolve_locked(&self, package: &LockedPackage) -> Result<ResolvedArtifact, String> {
        let LockedSource::Artifact {
            index_sha256,
            targets,
            ..
        } = &package.source
        else {
            unreachable!("locked_package only returns artifacts");
        };
        normalize_sha256(index_sha256, "locked artifact index checksum")?;
        validate_locked_targets(targets)?;
        let target_name = self.target.as_ref().map_err(Clone::clone)?;
        let target = targets.get(target_name).ok_or_else(|| {
            format!(
                "Artifact package '{}' has no locked archive for target '{}'; available targets: {}",
                package.name,
                target_name,
                available_targets(targets.keys())
            )
        })?;
        validate_format(&target.format)?;
        let archive_url = parse_source_url(&target.url, "locked archive")?;
        let checksum = normalize_sha256(&target.sha256, "locked archive checksum")?;
        let archive = self.cache()?.archive(&archive_url, &checksum)?;
        let root = self.cache()?.extracted_package(&archive, &checksum)?;
        Ok(ResolvedArtifact {
            root,
            version: package.version.clone(),
            dependencies: package.dependencies.clone(),
            source: package.source.clone(),
        })
    }

    fn resolve_unlocked(
        &self,
        expected_name: &str,
        index_url: &Url,
    ) -> Result<ResolvedArtifact, String> {
        let (index_bytes, index_sha256) = self.cache()?.fetch_index(index_url)?;
        let index: ArtifactIndex = toml::from_slice(&index_bytes).map_err(|error| {
            format!(
                "Invalid artifact index '{}': {error}",
                display_url(index_url)
            )
        })?;
        if index.version != INDEX_VERSION {
            return Err(format!(
                "Artifact index '{}' has unsupported version {}; expected {}",
                display_url(index_url),
                index.version,
                INDEX_VERSION
            ));
        }
        if index.package.name != expected_name {
            return Err(format!(
                "Dependency key '{expected_name}' does not match artifact index package name '{}'",
                index.package.name
            ));
        }
        if index.package.version.trim().is_empty() {
            return Err(format!(
                "Artifact index for package '{}' declares an empty version",
                index.package.name
            ));
        }

        let mut targets = BTreeMap::new();
        for (name, target) in index.targets {
            validate_target_name(&name)?;
            validate_format(&target.format)?;
            let checksum = normalize_sha256(
                &target.sha256,
                &format!("archive checksum for target '{name}'"),
            )?;
            let url = index_url.join(&target.url).map_err(|error| {
                format!(
                    "Invalid archive URL '{}' in artifact index '{}': {error}",
                    target.url,
                    display_url(index_url)
                )
            })?;
            validate_source_url(&url, "archive")?;
            targets.insert(
                name,
                LockedTarget {
                    url: url.to_string(),
                    sha256: checksum,
                    format: target.format,
                },
            );
        }
        let target_name = self.target.as_ref().map_err(Clone::clone)?;
        let target = targets.get(target_name).ok_or_else(|| {
            format!(
                "Artifact package '{}' has no archive for target '{}'; available targets: {}",
                index.package.name,
                target_name,
                available_targets(targets.keys())
            )
        })?;
        let archive_url = parse_source_url(&target.url, "archive")?;
        let archive = self.cache()?.archive(&archive_url, &target.sha256)?;
        let root = self.cache()?.extracted_package(&archive, &target.sha256)?;
        Ok(ResolvedArtifact {
            root,
            version: index.package.version,
            dependencies: index.dependencies,
            source: LockedSource::Artifact {
                index: index_url.to_string(),
                index_sha256,
                targets,
            },
        })
    }
}

fn read_lockfile(project_root: &Path) -> Result<ExistingLock, String> {
    let path = project_root.join("p7.lock");
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ExistingLock::None);
        }
        Err(error) => return Err(format!("Cannot read '{}': {error}", path.display())),
    };
    let value: toml::Value = toml::from_str(&source)
        .map_err(|error| format!("Invalid '{}': {error}", path.display()))?;
    let version = value
        .get("version")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| format!("Lockfile '{}' is missing integer version", path.display()))?;
    match version {
        1 => {
            let lock: V1Lockfile = toml::from_str(&source).map_err(|error| {
                format!("Invalid version 1 lockfile '{}': {error}", path.display())
            })?;
            validate_v1_lock(&lock, &path)?;
            Ok(ExistingLock::V1)
        }
        2 => {
            let lock: Lockfile = toml::from_str(&source).map_err(|error| {
                format!("Invalid version 2 lockfile '{}': {error}", path.display())
            })?;
            if lock.version != LOCK_VERSION {
                unreachable!("version was inspected above");
            }
            let mut packages = HashMap::new();
            for package in lock.package {
                let name = package.name.clone();
                if packages.insert(name.clone(), package).is_some() {
                    return Err(format!(
                        "Lockfile '{}' contains duplicate package '{name}'",
                        path.display()
                    ));
                }
            }
            Ok(ExistingLock::V2(packages))
        }
        other => Err(format!(
            "Lockfile '{}' has unsupported version {other}; supported versions are 1 and 2",
            path.display()
        )),
    }
}

fn validate_v1_lock(lock: &V1Lockfile, path: &Path) -> Result<(), String> {
    if lock.version != 1 {
        return Err(format!(
            "Invalid version 1 lockfile '{}': version is {}",
            path.display(),
            lock.version
        ));
    }
    let mut names = HashSet::new();
    for package in &lock.packages {
        if !names.insert(&package.name) {
            return Err(format!(
                "Lockfile '{}' contains duplicate package '{}'",
                path.display(),
                package.name
            ));
        }
        if !package.source.starts_with("path+") {
            return Err(format!(
                "Version 1 lockfile '{}' contains unsupported source '{}' for package '{}'",
                path.display(),
                package.source,
                package.name
            ));
        }
        let _ = (&package.version, &package.checksum, &package.dependencies);
    }
    Ok(())
}

fn default_cache_dir() -> Result<PathBuf, String> {
    ProjectDirs::from("", "", "protosept")
        .map(|dirs| dirs.cache_dir().join("artifacts-v1"))
        .ok_or_else(|| "Cannot determine the operating-system user cache directory".to_string())
}

pub(crate) fn host_target() -> Result<String, String> {
    canonical_target(std::env::consts::ARCH, std::env::consts::OS)
}

pub(crate) fn canonical_target(arch: &str, os: &str) -> Result<String, String> {
    match (arch, os) {
        ("aarch64", "macos") => Ok("aarch64-apple-darwin".to_string()),
        ("x86_64", "macos") => Ok("x86_64-apple-darwin".to_string()),
        ("x86_64", "linux") => Ok("x86_64-unknown-linux-gnu".to_string()),
        ("x86_64", "windows") => Ok("x86_64-pc-windows-msvc".to_string()),
        _ => Err(format!(
            "Artifact packages are not supported on host architecture '{arch}' and operating system '{os}'"
        )),
    }
}

fn validate_format(format: &str) -> Result<(), String> {
    if format == "tar.gz" {
        Ok(())
    } else {
        Err(format!(
            "Unsupported artifact archive format '{format}'; expected 'tar.gz'"
        ))
    }
}

fn validate_target_name(target: &str) -> Result<(), String> {
    if matches!(
        target,
        "aarch64-apple-darwin"
            | "x86_64-apple-darwin"
            | "x86_64-unknown-linux-gnu"
            | "x86_64-pc-windows-msvc"
    ) {
        Ok(())
    } else {
        Err(format!(
            "Unsupported artifact target '{target}'; expected one of the four canonical targets"
        ))
    }
}

fn validate_locked_targets(targets: &BTreeMap<String, LockedTarget>) -> Result<(), String> {
    for (name, target) in targets {
        validate_target_name(name)?;
        validate_format(&target.format)?;
        normalize_sha256(
            &target.sha256,
            &format!("locked archive checksum for target '{name}'"),
        )?;
        parse_source_url(&target.url, &format!("locked archive for target '{name}'"))?;
    }
    Ok(())
}

fn normalize_sha256(value: &str, field: &str) -> Result<String, String> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(format!("{field} must be a 64-digit SHA-256 value"))
    }
}

fn parse_source_url(value: &str, field: &str) -> Result<Url, String> {
    let url =
        Url::parse(value).map_err(|error| format!("Invalid {field} URL '{value}': {error}"))?;
    validate_source_url(&url, field)?;
    Ok(url)
}

fn validate_source_url(url: &Url, field: &str) -> Result<(), String> {
    match url.scheme() {
        "https" => Ok(()),
        "file" => {
            url.to_file_path().map_err(|_| {
                format!(
                    "Invalid {field} file URL '{}': it must identify an absolute local path",
                    display_url(url)
                )
            })?;
            Ok(())
        }
        scheme => Err(format!(
            "Unsupported {field} URL scheme '{scheme}' in '{}'; expected https or file",
            display_url(url)
        )),
    }
}

fn available_targets<'a>(targets: impl Iterator<Item = &'a String>) -> String {
    let values = targets.cloned().collect::<Vec<_>>();
    if values.is_empty() {
        "(none)".to_string()
    } else {
        values.join(", ")
    }
}

fn display_url(url: &Url) -> String {
    url.as_str().to_string()
}

struct Cache {
    root: PathBuf,
}

impl Cache {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn fetch_index(&self, url: &Url) -> Result<(Vec<u8>, String), String> {
        let directory = self.root.join("indexes");
        fs::create_dir_all(&directory).map_err(|error| {
            format!(
                "Cannot create cache directory '{}': {error}",
                directory.display()
            )
        })?;
        let temporary = temporary_path(&directory, "index");
        let result = (|| {
            let mut input = open_url(url, "artifact index")?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| {
                    format!(
                        "Cannot create temporary cache file '{}': {error}",
                        temporary.display()
                    )
                })?;
            let checksum = copy_and_hash(&mut input, &mut output).map_err(|error| {
                format!(
                    "Cannot download artifact index '{}': {error}",
                    display_url(url)
                )
            })?;
            output.sync_all().map_err(|error| {
                format!(
                    "Cannot sync temporary cache file '{}': {error}",
                    temporary.display()
                )
            })?;
            let destination = directory.join(format!("{checksum}.toml"));
            atomic_publish(&temporary, &destination)?;
            let actual = sha256_file(&destination)?;
            if actual != checksum {
                return Err(format!(
                    "Corrupted artifact index cache entry '{}': expected SHA-256 {checksum}, found {actual}",
                    destination.display()
                ));
            }
            let bytes = fs::read(&destination).map_err(|error| {
                format!(
                    "Cannot read artifact index cache entry '{}': {error}",
                    destination.display()
                )
            })?;
            Ok((bytes, checksum))
        })();
        if result.is_err() {
            fs::remove_file(&temporary).ok();
        }
        result
    }

    fn archive(&self, url: &Url, checksum: &str) -> Result<PathBuf, String> {
        let destination = self
            .root
            .join("archives")
            .join(format!("{checksum}.tar.gz"));
        if destination.exists() {
            let actual = sha256_file(&destination)?;
            if actual != checksum {
                return Err(format!(
                    "Corrupted artifact archive cache entry '{}': expected SHA-256 {checksum}, found {actual}",
                    destination.display()
                ));
            }
            return Ok(destination);
        }
        let parent = destination
            .parent()
            .expect("cache destination must have parent");
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Cannot create cache directory '{}': {error}",
                parent.display()
            )
        })?;
        let temporary = temporary_path(parent, "archive");
        let result = (|| {
            let mut input = open_url(url, "artifact archive")?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| {
                    format!(
                        "Cannot create temporary cache file '{}': {error}",
                        temporary.display()
                    )
                })?;
            let actual = copy_and_hash(&mut input, &mut output).map_err(|error| {
                format!(
                    "Cannot download artifact archive '{}': {error}",
                    display_url(url)
                )
            })?;
            output.sync_all().map_err(|error| {
                format!(
                    "Cannot sync temporary cache file '{}': {error}",
                    temporary.display()
                )
            })?;
            if actual != checksum {
                return Err(format!(
                    "Checksum mismatch for artifact archive '{}': expected {checksum}, found {actual}",
                    display_url(url)
                ));
            }
            atomic_publish(&temporary, &destination)?;
            let published = sha256_file(&destination)?;
            if published != checksum {
                return Err(format!(
                    "Corrupted artifact archive cache entry '{}': expected SHA-256 {checksum}, found {published}",
                    destination.display()
                ));
            }
            Ok(destination.clone())
        })();
        if result.is_err() {
            fs::remove_file(&temporary).ok();
        }
        result
    }

    fn extracted_package(&self, archive: &Path, checksum: &str) -> Result<PathBuf, String> {
        let destination = self.root.join("packages").join(checksum);
        if destination.exists() {
            validate_cached_package(&destination, checksum)?;
            return Ok(destination);
        }
        let parent = destination
            .parent()
            .expect("package cache destination must have parent");
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Cannot create cache directory '{}': {error}",
                parent.display()
            )
        })?;
        let temporary = temporary_path(parent, "package");
        fs::create_dir(&temporary).map_err(|error| {
            format!(
                "Cannot create temporary package directory '{}': {error}",
                temporary.display()
            )
        })?;
        let result = (|| {
            extract_tar_gz(archive, &temporary)?;
            let tree_checksum = package_tree_checksum(&temporary)?;
            fs::write(
                temporary.join(CACHE_MARKER),
                format!("{checksum}\n{tree_checksum}\n"),
            )
            .map_err(|error| {
                format!(
                    "Cannot write artifact cache marker in '{}': {error}",
                    temporary.display()
                )
            })?;
            match fs::rename(&temporary, &destination) {
                Ok(()) => Ok(destination.clone()),
                Err(_) if destination.exists() => {
                    fs::remove_dir_all(&temporary).ok();
                    validate_cached_package(&destination, checksum)?;
                    Ok(destination.clone())
                }
                Err(error) => Err(format!(
                    "Cannot publish extracted package cache '{}': {error}",
                    destination.display()
                )),
            }
        })();
        if result.is_err() {
            fs::remove_dir_all(&temporary).ok();
        }
        result
    }
}

fn open_url(url: &Url, description: &str) -> Result<Box<dyn Read>, String> {
    match url.scheme() {
        "file" => {
            let path = url
                .to_file_path()
                .map_err(|_| format!("Invalid {description} file URL '{}'", display_url(url)))?;
            File::open(&path)
                .map(|file| Box::new(file) as Box<dyn Read>)
                .map_err(|error| {
                    format!(
                        "Cannot open {description} '{}' from '{}': {error}",
                        path.display(),
                        display_url(url)
                    )
                })
        }
        "https" => {
            let response = reqwest::blocking::Client::builder()
                .https_only(true)
                .build()
                .map_err(|error| format!("Cannot initialize HTTPS client: {error}"))?
                .get(url.clone())
                .send()
                .and_then(reqwest::blocking::Response::error_for_status)
                .map_err(|error| {
                    format!(
                        "Cannot download {description} '{}': {error}",
                        display_url(url)
                    )
                })?;
            Ok(Box::new(response))
        }
        _ => unreachable!("URLs are validated before opening"),
    }
}

fn atomic_publish(temporary: &Path, destination: &Path) -> Result<(), String> {
    match fs::rename(temporary, destination) {
        Ok(()) => Ok(()),
        Err(_) if destination.exists() => {
            fs::remove_file(temporary).ok();
            Ok(())
        }
        Err(error) => Err(format!(
            "Cannot publish cache entry '{}': {error}",
            destination.display()
        )),
    }
}

fn validate_cached_package(path: &Path, checksum: &str) -> Result<(), String> {
    let marker = fs::read_to_string(path.join(CACHE_MARKER)).map_err(|error| {
        format!(
            "Corrupted extracted package cache entry '{}': cannot read marker: {error}",
            path.display()
        )
    })?;
    let mut lines = marker.lines();
    let marked_archive = lines.next();
    let marked_tree = lines.next();
    if marked_archive != Some(checksum)
        || marked_tree.is_none()
        || lines.next().is_some()
        || !path.join("p7.toml").is_file()
    {
        return Err(format!(
            "Corrupted extracted package cache entry '{}': marker or p7.toml is invalid",
            path.display()
        ));
    }
    let actual_tree = package_tree_checksum(path)?;
    if marked_tree != Some(actual_tree.as_str()) {
        return Err(format!(
            "Corrupted extracted package cache entry '{}': extracted contents do not match the cache marker",
            path.display()
        ));
    }
    Ok(())
}

fn package_tree_checksum(root: &Path) -> Result<String, String> {
    fn collect(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(directory).map_err(|error| {
            format!(
                "Cannot inspect extracted package cache '{}': {error}",
                directory.display()
            )
        })? {
            let entry = entry.map_err(|error| {
                format!(
                    "Cannot inspect extracted package cache '{}': {error}",
                    directory.display()
                )
            })?;
            let path = entry.path();
            if path == root.join(CACHE_MARKER) {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                format!(
                    "Cannot inspect extracted package cache '{}': {error}",
                    path.display()
                )
            })?;
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "Corrupted extracted package cache entry '{}': symbolic links are forbidden",
                    path.display()
                ));
            }
            if metadata.is_dir() {
                collect(root, &path, files)?;
            } else if metadata.is_file() {
                files.push(path);
            } else {
                return Err(format!(
                    "Corrupted extracted package cache entry '{}': special files are forbidden",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for file in files {
        let relative = file.strip_prefix(root).map_err(|_| {
            format!(
                "Cannot checksum extracted package path '{}'",
                file.display()
            )
        })?;
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        let contents = fs::read(&file).map_err(|error| {
            format!(
                "Cannot checksum extracted package file '{}': {error}",
                file.display()
            )
        })?;
        hasher.update((contents.len() as u64).to_le_bytes());
        hasher.update(contents);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn extract_tar_gz(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(archive_path)
        .map_err(|error| format!("Cannot open archive '{}': {error}", archive_path.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive.entries().map_err(|error| {
        format!(
            "Invalid tar.gz archive '{}': {error}",
            archive_path.display()
        )
    })?;
    let mut paths = HashSet::new();
    let mut manifest_count = 0;
    for entry in entries {
        let mut entry = entry.map_err(|error| {
            format!(
                "Invalid tar.gz archive '{}': {error}",
                archive_path.display()
            )
        })?;
        let path = entry.path().map_err(|error| {
            format!(
                "Invalid path in tar.gz archive '{}': {error}",
                archive_path.display()
            )
        })?;
        validate_archive_path(&path)?;
        let path = path.into_owned();
        if !paths.insert(path.clone()) {
            return Err(format!(
                "Unsafe tar.gz archive '{}': duplicate path '{}'",
                archive_path.display(),
                path.display()
            ));
        }
        let entry_type = entry.header().entry_type();
        if path == Path::new("p7.toml") {
            manifest_count += 1;
            if !entry_type.is_file() {
                return Err("Artifact archive p7.toml must be a regular file".to_string());
            }
        }
        let output = destination.join(&path);
        if entry_type == EntryType::Directory {
            fs::create_dir_all(&output).map_err(|error| {
                format!(
                    "Cannot create extracted directory '{}': {error}",
                    output.display()
                )
            })?;
        } else if entry_type.is_file() {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    format!(
                        "Cannot create extracted directory '{}': {error}",
                        parent.display()
                    )
                })?;
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .map_err(|error| format!("Cannot extract '{}': {error}", output.display()))?;
            std::io::copy(&mut entry, &mut file)
                .map_err(|error| format!("Cannot extract '{}': {error}", output.display()))?;
        } else {
            return Err(format!(
                "Unsafe tar.gz archive '{}': path '{}' has unsupported entry type {:?}; links and special files are forbidden",
                archive_path.display(),
                path.display(),
                entry_type
            ));
        }
    }
    if manifest_count != 1 {
        return Err(format!(
            "Artifact archive '{}' must contain exactly one root p7.toml",
            archive_path.display()
        ));
    }
    Ok(())
}

fn validate_archive_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.to_string_lossy().contains('\\')
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "Unsafe artifact archive path '{}': paths must be relative and cannot contain '..' or backslashes",
            path.display()
        ));
    }
    Ok(())
}

fn temporary_path(parent: &Path, label: &str) -> PathBuf {
    let value = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(".{label}-{}-{value}.partial", std::process::id()))
}

fn copy_and_hash(input: &mut dyn Read, output: &mut dyn Write) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        output.write_all(&buffer[..count])?;
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("Cannot read cache entry '{}': {error}", path.display()))?;
    let mut sink = std::io::sink();
    copy_and_hash(&mut file, &mut sink)
        .map_err(|error| format!("Cannot checksum cache entry '{}': {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_targets_cover_supported_hosts() {
        assert_eq!(
            canonical_target("aarch64", "macos").unwrap(),
            "aarch64-apple-darwin"
        );
        assert_eq!(
            canonical_target("x86_64", "macos").unwrap(),
            "x86_64-apple-darwin"
        );
        assert_eq!(
            canonical_target("x86_64", "linux").unwrap(),
            "x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            canonical_target("x86_64", "windows").unwrap(),
            "x86_64-pc-windows-msvc"
        );
        assert!(canonical_target("aarch64", "linux").is_err());
    }
}
