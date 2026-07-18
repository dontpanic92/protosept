use crate::artifact::{ArtifactResolver, LockedPackage, LockedSource, Lockfile};
use p7::ModuleProvider;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

const MANIFEST_NAME: &str = "p7.toml";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub package: PackageManifest,
    #[serde(default)]
    pub dependencies: BTreeMap<String, DependencyManifest>,
    #[serde(default)]
    pub native: NativeManifest,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub kind: PackageKind,
    #[serde(default = "default_source")]
    pub source: PathBuf,
    pub entry: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PackageKind {
    Library,
    #[default]
    Executable,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum DependencyManifest {
    Path(PathDependency),
    Artifact(ArtifactDependency),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PathDependency {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDependency {
    pub index: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct NativeManifest {
    #[serde(default)]
    pub extensions: Vec<PathBuf>,
}

fn default_source() -> PathBuf {
    PathBuf::from("src")
}

impl PackageKind {
    fn default_entry(self) -> PathBuf {
        match self {
            Self::Library => PathBuf::from("src/mod.p7"),
            Self::Executable => PathBuf::from("src/main.p7"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Package {
    pub manifest: Manifest,
    pub root: PathBuf,
    pub source_dir: PathBuf,
    pub entry_path: PathBuf,
    pub entry_module: String,
    source: LockedSource,
}

#[derive(Debug, Clone)]
pub struct Project {
    root_package: String,
    packages: HashMap<String, Package>,
}

impl Project {
    pub fn load(path: &Path) -> Result<Self, String> {
        let manifest_path = manifest_path(path)?;
        let project_root = manifest_path
            .parent()
            .expect("manifest path must have parent");
        let mut resolver = ArtifactResolver::for_project(project_root)?;
        Self::load_with_resolver(&manifest_path, &mut resolver)
    }

    fn load_with_resolver(
        manifest_path: &Path,
        resolver: &mut ArtifactResolver,
    ) -> Result<Self, String> {
        let mut packages = HashMap::new();
        let mut loading = HashSet::new();
        let root_package = load_package_recursive(
            manifest_path,
            &mut packages,
            &mut loading,
            None,
            LockedSource::Path {
                path: ".".to_string(),
            },
            false,
            resolver,
        )?;
        Ok(Self {
            root_package,
            packages,
        })
    }

    #[cfg(test)]
    fn load_for_test(path: &Path, cache_dir: PathBuf, target: &str) -> Result<Self, String> {
        let manifest_path = manifest_path(path)?;
        let project_root = manifest_path
            .parent()
            .expect("manifest path must have parent");
        let mut resolver = ArtifactResolver::for_test(project_root, cache_dir, target)?;
        Self::load_with_resolver(&manifest_path, &mut resolver)
    }

    pub fn root_package(&self) -> &Package {
        self.packages
            .get(&self.root_package)
            .expect("loaded project must contain root package")
    }

    pub fn compile(&self) -> Result<p7::bytecode::Module, p7::errors::Proto7Error> {
        let package = self.root_package();
        let source = fs::read_to_string(&package.entry_path).map_err(|error| {
            p7::errors::Proto7Error::SemanticError(p7::errors::SemanticError::Other(format!(
                "Cannot read package entry '{}': {error}",
                package.entry_path.display()
            )))
        })?;
        p7::compile_module_with_provider(
            source,
            &package.entry_module,
            Box::new(ProjectModuleProvider::new(self.clone())),
        )
    }

    pub fn validate_supported_features(&self) -> Result<(), String> {
        for package in self.packages.values() {
            for extension in &package.manifest.native.extensions {
                let path = normalize_under_root(&package.root, extension, "native extension")?;
                if !path.is_file() {
                    return Err(format!(
                        "Package '{}' native extension '{}' does not exist",
                        package.manifest.package.name,
                        path.display()
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn load_native_extensions(
        &self,
        runtime: &mut p7::embedding::Runtime,
    ) -> Result<(), String> {
        for package_name in self.package_load_order()? {
            let package = self
                .packages
                .get(&package_name)
                .expect("load order only contains loaded packages");
            for extension in &package.manifest.native.extensions {
                let path = normalize_under_root(&package.root, extension, "native extension")?;
                runtime
                    .load_native_extension(&path)
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    fn package_load_order(&self) -> Result<Vec<String>, String> {
        fn visit(
            name: &str,
            packages: &HashMap<String, Package>,
            visiting: &mut HashSet<String>,
            visited: &mut HashSet<String>,
            order: &mut Vec<String>,
        ) -> Result<(), String> {
            if visited.contains(name) {
                return Ok(());
            }
            if !visiting.insert(name.to_string()) {
                return Err(format!("Cyclic package dependency through '{name}'"));
            }
            let package = packages
                .get(name)
                .ok_or_else(|| format!("Unknown package '{name}'"))?;
            for dependency in package.manifest.dependencies.keys() {
                visit(dependency, packages, visiting, visited, order)?;
            }
            visiting.remove(name);
            visited.insert(name.to_string());
            order.push(name.to_string());
            Ok(())
        }

        let mut order = Vec::new();
        visit(
            &self.root_package,
            &self.packages,
            &mut HashSet::new(),
            &mut HashSet::new(),
            &mut order,
        )?;
        Ok(order)
    }

    pub fn write_lockfile(&self) -> Result<PathBuf, String> {
        let project_root = &self.root_package().root;
        let mut packages = self
            .packages
            .values()
            .map(|package| {
                Ok(LockedPackage {
                    name: package.manifest.package.name.clone(),
                    version: package.manifest.package.version.clone(),
                    source: match &package.source {
                        LockedSource::Path { .. } => LockedSource::Path {
                            path: portable_relative_path(&relative_path(
                                project_root,
                                &package.root,
                            ))?,
                        },
                        source @ LockedSource::Artifact { .. } => source.clone(),
                    },
                    checksum: matches!(package.source, LockedSource::Path { .. })
                        .then(|| package_checksum(package))
                        .transpose()?,
                    dependencies: package.manifest.dependencies.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        packages.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));
        let lockfile = Lockfile::new(packages);
        let encoded = toml::to_string_pretty(&lockfile)
            .map_err(|error| format!("Cannot encode lockfile: {error}"))?;
        let path = self.root_package().root.join("p7.lock");
        if fs::read_to_string(&path).is_ok_and(|existing| existing == encoded) {
            return Ok(path);
        }
        let temporary = self
            .root_package()
            .root
            .join(format!(".p7.lock-{}.partial", std::process::id()));
        fs::write(&temporary, encoded)
            .map_err(|error| format!("Cannot write '{}': {error}", temporary.display()))?;
        if let Err(first_error) = fs::rename(&temporary, &path) {
            let retry = if path.exists() {
                fs::remove_file(&path).and_then(|()| fs::rename(&temporary, &path))
            } else {
                Err(first_error.kind().into())
            };
            if let Err(retry_error) = retry {
                fs::remove_file(&temporary).ok();
                return Err(format!(
                    "Cannot publish '{}': {first_error}; replacement failed: {retry_error}",
                    path.display()
                ));
            }
        }
        Ok(path)
    }

    #[cfg(test)]
    pub fn package_names(&self) -> impl Iterator<Item = &str> {
        self.packages.keys().map(String::as_str)
    }

    pub fn test_module_name(&self, file: &Path) -> Result<String, String> {
        let package = self.root_package();
        let tests_dir = package.root.join("tests");
        let relative = file.strip_prefix(&tests_dir).map_err(|_| {
            format!(
                "Test '{}' is outside package test directory '{}'",
                file.display(),
                tests_dir.display()
            )
        })?;
        let stem = relative
            .with_extension("")
            .components()
            .filter_map(|component| match component {
                Component::Normal(value) => value.to_str().map(str::to_string),
                _ => None,
            })
            .collect::<Vec<_>>();
        for segment in &stem {
            validate_package_name(segment)?;
        }
        Ok(std::iter::once(package.manifest.package.name.clone())
            .chain(std::iter::once("__tests__".to_string()))
            .chain(stem)
            .collect::<Vec<_>>()
            .join("."))
    }
}

#[derive(Clone)]
pub struct ProjectModuleProvider {
    project: Rc<Project>,
    source_cache: Rc<std::cell::RefCell<HashMap<PathBuf, String>>>,
}

impl ProjectModuleProvider {
    pub fn new(project: Project) -> Self {
        Self {
            project: Rc::new(project),
            source_cache: Rc::new(std::cell::RefCell::new(HashMap::new())),
        }
    }

    fn module_file(&self, module_path: &str) -> Option<PathBuf> {
        let mut segments = module_path.split('.');
        let package_name = segments.next()?;
        let package = self.project.packages.get(package_name)?;
        if module_path == package.entry_module {
            return Some(package.entry_path.clone());
        }
        let relative: Vec<&str> = segments.collect();

        if relative.is_empty() {
            return Some(package.source_dir.join("mod.p7"));
        }

        let mut path = package.source_dir.clone();
        for segment in &relative[..relative.len() - 1] {
            path.push(segment);
        }
        let leaf = relative.last()?;
        let module_file = path.join(format!("{leaf}.p7"));
        if module_file.is_file() {
            Some(module_file)
        } else {
            Some(path.join(leaf).join("mod.p7"))
        }
    }

    fn read_source(&self, path: &Path) -> Option<String> {
        if let Some(source) = self.source_cache.borrow().get(path) {
            return Some(source.clone());
        }
        let source = fs::read_to_string(path).ok()?;
        self.source_cache
            .borrow_mut()
            .insert(path.to_path_buf(), source.clone());
        Some(source)
    }

    fn can_import(&self, requester: &str, target: &str) -> bool {
        let Some(requester_package) = requester.split('.').next() else {
            return false;
        };
        let Some(target_package) = target.split('.').next() else {
            return false;
        };
        if requester_package == target_package {
            return true;
        }
        self.project
            .packages
            .get(requester_package)
            .is_some_and(|package| package.manifest.dependencies.contains_key(target_package))
    }
}

impl ModuleProvider for ProjectModuleProvider {
    fn load_module(&self, module_path: &str) -> Option<String> {
        let path = self.module_file(module_path)?;
        self.read_source(&path)
    }

    fn clone_boxed(&self) -> Box<dyn ModuleProvider> {
        Box::new(self.clone())
    }

    fn load_module_from(&self, requester: &str, module_path: &str) -> Option<String> {
        self.can_import(requester, module_path)
            .then(|| self.load_module(module_path))
            .flatten()
    }

    fn module_is_directory(&self, module_path: &str) -> bool {
        self.module_file(module_path)
            .and_then(|path| path.file_name().map(|name| name == "mod.p7"))
            .unwrap_or(false)
    }
}

fn manifest_path(path: &Path) -> Result<PathBuf, String> {
    let candidate = if path.file_name().is_some_and(|name| name == MANIFEST_NAME) {
        path.to_path_buf()
    } else {
        path.join(MANIFEST_NAME)
    };
    candidate
        .canonicalize()
        .map_err(|error| format!("Cannot find '{}': {error}", candidate.display()))
}

fn load_package_recursive(
    manifest_path: &Path,
    packages: &mut HashMap<String, Package>,
    loading: &mut HashSet<PathBuf>,
    expected_name: Option<&str>,
    package_source: LockedSource,
    downloaded: bool,
    resolver: &mut ArtifactResolver,
) -> Result<String, String> {
    let manifest_path = manifest_path
        .canonicalize()
        .map_err(|error| format!("Cannot resolve '{}': {error}", manifest_path.display()))?;
    if !loading.insert(manifest_path.clone()) {
        return Err(format!(
            "Cyclic package dependency through '{}'",
            manifest_path.display()
        ));
    }

    let result = (|| {
        let source = fs::read_to_string(&manifest_path)
            .map_err(|error| format!("Cannot read '{}': {error}", manifest_path.display()))?;
        let manifest: Manifest = toml::from_str(&source)
            .map_err(|error| format!("Invalid '{}': {error}", manifest_path.display()))?;
        validate_package_name(&manifest.package.name)?;
        if manifest.package.version.trim().is_empty() {
            return Err(format!(
                "Package '{}' must declare a non-empty version",
                manifest.package.name
            ));
        }
        if let Some(expected) = expected_name
            && expected != manifest.package.name
        {
            return Err(format!(
                "Dependency key '{}' does not match package name '{}'",
                expected, manifest.package.name
            ));
        }
        let root = manifest_path
            .parent()
            .expect("manifest path must have parent")
            .to_path_buf();
        let source_dir = normalize_under_root(&root, &manifest.package.source, "source")?;
        let entry = manifest
            .package
            .entry
            .clone()
            .unwrap_or_else(|| manifest.package.kind.default_entry());
        let entry_path = normalize_under_root(&root, &entry, "entry")?;
        if !entry_path.starts_with(&source_dir) {
            return Err(format!(
                "Package '{}' entry '{}' must be inside source directory '{}'",
                manifest.package.name,
                entry_path.display(),
                source_dir.display()
            ));
        }
        if !entry_path.is_file() {
            return Err(format!(
                "Package '{}' entry '{}' does not exist",
                manifest.package.name,
                entry_path.display()
            ));
        }
        let entry_module = module_name_for_file(&manifest.package.name, &source_dir, &entry_path)?;

        if let Some(existing) = packages.get(&manifest.package.name) {
            if existing.root != root {
                return Err(format!(
                    "Package name '{}' resolves to both '{}' and '{}'",
                    manifest.package.name,
                    existing.root.display(),
                    root.display()
                ));
            }
            match (&existing.source, &package_source) {
                (
                    LockedSource::Artifact {
                        index: existing, ..
                    },
                    LockedSource::Artifact { index: current, .. },
                ) if existing != current => {
                    return Err(format!(
                        "Package '{}' resolves through conflicting artifact indexes '{}' and '{}'",
                        manifest.package.name, existing, current
                    ));
                }
                (LockedSource::Path { .. }, LockedSource::Artifact { .. })
                | (LockedSource::Artifact { .. }, LockedSource::Path { .. }) => {
                    return Err(format!(
                        "Package '{}' resolves as both a path and artifact package",
                        manifest.package.name
                    ));
                }
                _ => {}
            }
            return Ok(manifest.package.name);
        }

        for (dependency_name, dependency) in &manifest.dependencies {
            validate_package_name(dependency_name)?;
            match dependency {
                DependencyManifest::Path(dependency) => {
                    if downloaded {
                        return Err(format!(
                            "Downloaded artifact package '{}' cannot use path dependency '{}'",
                            manifest.package.name, dependency_name
                        ));
                    }
                    let dependency_manifest = root.join(&dependency.path).join(MANIFEST_NAME);
                    load_package_recursive(
                        &dependency_manifest,
                        packages,
                        loading,
                        Some(dependency_name),
                        LockedSource::Path {
                            path: dependency.path.to_string_lossy().into_owned(),
                        },
                        false,
                        resolver,
                    )?;
                }
                DependencyManifest::Artifact(dependency) => {
                    let resolved = resolver.resolve(dependency_name, &dependency.index)?;
                    let dependency_manifest = resolved.root.join(MANIFEST_NAME);
                    let dependency_source = resolved.source.clone();
                    let loaded_name = load_package_recursive(
                        &dependency_manifest,
                        packages,
                        loading,
                        Some(dependency_name),
                        dependency_source,
                        true,
                        resolver,
                    )?;
                    let loaded = packages
                        .get(&loaded_name)
                        .expect("recursive load must insert package");
                    if loaded.manifest.package.version != resolved.version {
                        return Err(format!(
                            "Artifact package '{}' metadata mismatch: index or lockfile version is '{}', archive p7.toml version is '{}'",
                            dependency_name, resolved.version, loaded.manifest.package.version
                        ));
                    }
                    if loaded.manifest.dependencies != resolved.dependencies {
                        return Err(format!(
                            "Artifact package '{}' metadata mismatch: archive dependency sources do not match the artifact index or lockfile",
                            dependency_name
                        ));
                    }
                }
            }
        }

        let name = manifest.package.name.clone();
        packages.insert(
            name.clone(),
            Package {
                manifest,
                root,
                source_dir,
                entry_path,
                entry_module,
                source: package_source,
            },
        );
        Ok(name)
    })();

    loading.remove(&manifest_path);
    result
}

fn validate_package_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let valid_start = chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic());
    let valid_rest = chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric());
    if !valid_start || !valid_rest || matches!(name, "_" | "builtin") {
        return Err(format!(
            "Invalid package name '{name}'; use a Protosept identifier"
        ));
    }
    Ok(())
}

fn normalize_under_root(root: &Path, relative: &Path, field: &str) -> Result<PathBuf, String> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(format!(
            "Package {field} path '{}' must stay inside the package root",
            relative.display()
        ));
    }
    Ok(root.join(relative))
}

fn module_name_for_file(
    package_name: &str,
    source_dir: &Path,
    file: &Path,
) -> Result<String, String> {
    let relative = file
        .strip_prefix(source_dir)
        .map_err(|_| format!("Entry '{}' is outside the source directory", file.display()))?;
    if relative.extension().and_then(|ext| ext.to_str()) != Some("p7") {
        return Err(format!("Entry '{}' must end with .p7", file.display()));
    }

    let mut segments: Vec<String> = relative
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str().map(str::to_string),
            _ => None,
        })
        .collect();
    let stem = relative
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| format!("Invalid entry filename '{}'", file.display()))?;
    if stem != "mod" {
        segments.push(stem.to_string());
    }
    for segment in &segments {
        validate_package_name(segment)?;
    }
    Ok(std::iter::once(package_name.to_string())
        .chain(segments)
        .collect::<Vec<_>>()
        .join("."))
}

fn relative_path(base: &Path, target: &Path) -> PathBuf {
    let base_components = base.components().collect::<Vec<_>>();
    let target_components = target.components().collect::<Vec<_>>();
    let common = base_components
        .iter()
        .zip(&target_components)
        .take_while(|(lhs, rhs)| lhs == rhs)
        .count();

    let mut relative = PathBuf::new();
    for _ in common..base_components.len() {
        relative.push("..");
    }
    for component in &target_components[common..] {
        relative.push(component.as_os_str());
    }
    if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative
    }
}

fn portable_relative_path(path: &Path) -> Result<String, String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => parts.push(".".to_string()),
            Component::ParentDir => parts.push("..".to_string()),
            Component::Normal(value) => parts.push(
                value
                    .to_str()
                    .ok_or_else(|| format!("Package path '{}' is not valid UTF-8", path.display()))?
                    .to_string(),
            ),
            Component::Prefix(_) | Component::RootDir => {
                return Err(format!(
                    "Package path '{}' must be relative",
                    path.display()
                ));
            }
        }
    }
    if parts.is_empty() {
        Ok(".".to_string())
    } else {
        Ok(parts.join("/"))
    }
}

fn package_checksum(package: &Package) -> Result<String, String> {
    let mut files = vec![package.root.join(MANIFEST_NAME)];
    collect_source_files(&package.source_dir, &mut files)?;
    let mut files = files
        .into_iter()
        .map(|file| {
            let relative = file.strip_prefix(&package.root).map_err(|_| {
                format!("Cannot checksum '{}' outside package root", file.display())
            })?;
            Ok((portable_relative_path(relative)?, file))
        })
        .collect::<Result<Vec<_>, String>>()?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();
    for (relative, file) in files {
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        let contents = fs::read(&file)
            .map_err(|error| format!("Cannot checksum '{}': {error}", file.display()))?;
        hasher.update((contents.len() as u64).to_le_bytes());
        hasher.update(contents);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_source_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| {
        format!(
            "Cannot read source directory '{}': {error}",
            directory.display()
        )
    })? {
        let entry = entry.map_err(|error| {
            format!(
                "Cannot read source directory entry in '{}': {error}",
                directory.display()
            )
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_source_files(&path, files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("p7") {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use p7::interpreter::context::{Context, Data};
    use std::io::Cursor;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tar::{Builder, EntryType, Header};
    use url::Url;

    #[test]
    fn lockfile_paths_use_portable_separators() {
        assert_eq!(
            portable_relative_path(Path::new("../library/src/mod.p7")),
            Ok("../library/src/mod.p7".to_string())
        );
        assert_eq!(portable_relative_path(Path::new(".")), Ok(".".to_string()));
    }

    #[test]
    fn loads_path_dependency_and_package_relative_imports() {
        let root = temp_dir("project-load");
        let app = root.join("app");
        let dep = root.join("math");
        write(
            &app.join("p7.toml"),
            r#"
[package]
name = "app"
version = "0.1.0"
entry = "src/features/main.p7"

[dependencies]
math = { path = "../math" }
"#,
        );
        write(
            &app.join("src/features/main.p7"),
            r#"
import .helper;
import _.shared;
import math.answer;

pub fn main() -> int {
    helper.value() + shared.value() + answer.value()
}
"#,
        );
        write(
            &app.join("src/features/helper.p7"),
            "pub fn value() -> int { 1 }",
        );
        write(&app.join("src/shared.p7"), "pub fn value() -> int { 2 }");
        write(
            &dep.join("p7.toml"),
            r#"
[package]
name = "math"
version = "1.0.0"
"#,
        );
        write(&dep.join("src/main.p7"), "pub fn main() -> int { 0 }");
        write(&dep.join("src/answer.p7"), "pub fn value() -> int { 39 }");

        let project = Project::load(&app).expect("load project");
        assert_eq!(project.root_package().entry_module, "app.features.main");
        assert_eq!(project.package_names().count(), 2);
        let module = project.compile().expect("compile project");
        let mut context = Context::new();
        context.load_module(module);
        context.push_function("main", Vec::new());
        context.resume().expect("run project");
        assert_eq!(context.stack[0].stack.pop(), Some(Data::Int(42)));
        let lockfile = project.write_lockfile().expect("write lockfile");
        let lock_contents = fs::read_to_string(lockfile).expect("read lockfile");
        assert!(lock_contents.contains("name = \"app\""));
        assert!(lock_contents.contains("name = \"math\""));
        assert!(lock_contents.contains("checksum = \""));
        assert!(lock_contents.contains("version = 2"));
        assert!(lock_contents.contains("kind = \"path\""));
        assert!(lock_contents.contains("path = \".\""));
        assert!(lock_contents.contains("path = \"../math\""));
        assert!(!lock_contents.contains(&root.to_string_lossy().to_string()));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn package_kinds_select_default_root_modules() {
        let root = temp_dir("package-kinds");
        let library = root.join("library");
        let executable = root.join("executable");
        let legacy = root.join("legacy");
        write(
            &library.join("p7.toml"),
            "[package]\nname = \"library\"\nversion = \"0.1.0\"\nkind = \"library\"\n",
        );
        write(&library.join("src/mod.p7"), "pub fn answer() -> int { 42 }");
        write(
            &executable.join("p7.toml"),
            "[package]\nname = \"executable\"\nversion = \"0.1.0\"\nkind = \"executable\"\n",
        );
        write(&executable.join("src/main.p7"), "fn main() -> int { 0 }");
        write(
            &legacy.join("p7.toml"),
            "[package]\nname = \"legacy\"\nversion = \"0.1.0\"\n",
        );
        write(&legacy.join("src/main.p7"), "fn main() -> int { 0 }");

        let library_project = Project::load(&library).expect("load library");
        assert_eq!(
            library_project.root_package().manifest.package.kind,
            PackageKind::Library
        );
        assert_eq!(library_project.root_package().entry_module, "library");
        library_project.compile().expect("compile library");

        let executable_project = Project::load(&executable).expect("load executable");
        assert_eq!(
            executable_project.root_package().manifest.package.kind,
            PackageKind::Executable
        );
        assert_eq!(
            executable_project.root_package().entry_module,
            "executable.main"
        );

        let legacy_project = Project::load(&legacy).expect("load legacy executable");
        assert_eq!(
            legacy_project.root_package().manifest.package.kind,
            PackageKind::Executable
        );
        assert_eq!(legacy_project.root_package().entry_module, "legacy.main");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn executable_calls_library_root_module() {
        let root = temp_dir("library-dependency");
        let app = root.join("app");
        let library = root.join("library");
        write(
            &library.join("p7.toml"),
            "[package]\nname = \"library\"\nversion = \"0.1.0\"\nkind = \"library\"\n",
        );
        write(&library.join("src/mod.p7"), "pub fn answer() -> int { 42 }");
        write(
            &app.join("p7.toml"),
            r#"
[package]
name = "app"
version = "0.1.0"
kind = "executable"

[dependencies]
library = { path = "../library" }
"#,
        );
        write(
            &app.join("src/main.p7"),
            "import library; fn main() -> int { library.answer() }",
        );

        let project = Project::load(&app).expect("load executable");
        let module = project.compile().expect("compile executable");
        let mut context = Context::new();
        context.load_module(module);
        context.push_function("main", Vec::new());
        context.resume().expect("run executable");
        assert_eq!(context.stack[0].stack.pop(), Some(Data::Int(42)));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn native_extensions_load_dependencies_before_the_root() {
        let root = temp_dir("native-order");
        let app = root.join("app");
        let library = root.join("library");
        write(
            &library.join("p7.toml"),
            r#"
[package]
name = "library"
version = "0.1.0"
kind = "library"

[native]
extensions = ["native/library.so"]
"#,
        );
        write(&library.join("src/mod.p7"), "pub fn value() -> int { 1 }");
        write(&library.join("native/library.so"), "");
        write(
            &app.join("p7.toml"),
            r#"
[package]
name = "app"
version = "0.1.0"

[dependencies]
library = { path = "../library" }

[native]
extensions = ["native/app.so"]
"#,
        );
        write(&app.join("src/main.p7"), "fn main() -> int { 0 }");
        write(&app.join("native/app.so"), "");

        let project = Project::load(&app).expect("load project");
        project
            .validate_supported_features()
            .expect("validate extension paths");
        assert_eq!(
            project.package_load_order(),
            Ok(vec!["library".into(), "app".into()])
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn native_extensions_must_stay_inside_the_package() {
        let root = temp_dir("native-path");
        write(
            &root.join("p7.toml"),
            r#"
[package]
name = "app"
version = "0.1.0"

[native]
extensions = ["../outside.so"]
"#,
        );
        write(&root.join("src/main.p7"), "fn main() -> int { 0 }");

        let project = Project::load(&root).expect("load project");
        let error = project
            .validate_supported_features()
            .expect_err("extension path must not escape");
        assert!(error.contains("must stay inside the package root"));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn dependency_key_must_match_declared_package_name() {
        let root = temp_dir("dependency-name");
        let app = root.join("app");
        let dep = root.join("dep");
        write(
            &app.join("p7.toml"),
            r#"
[package]
name = "app"
version = "0.1.0"

[dependencies]
alias = { path = "../dep" }
"#,
        );
        write(&app.join("src/main.p7"), "fn main() {}");
        write(
            &dep.join("p7.toml"),
            r#"
[package]
name = "actual"
version = "0.1.0"
"#,
        );
        write(&dep.join("src/main.p7"), "fn main() {}");

        let error = Project::load(&app).expect_err("alias mismatch should fail");
        assert!(error.contains("does not match package name"));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn undeclared_transitive_dependency_is_not_visible() {
        let root = temp_dir("transitive-visibility");
        let app = root.join("app");
        let middle = root.join("middle");
        let leaf = root.join("leaf");
        write(
            &app.join("p7.toml"),
            r#"
[package]
name = "app"
version = "0.1.0"

[dependencies]
middle = { path = "../middle" }
"#,
        );
        write(
            &app.join("src/main.p7"),
            "import leaf.value; fn main() -> int { value.get() }",
        );
        write(
            &middle.join("p7.toml"),
            r#"
[package]
name = "middle"
version = "0.1.0"

[dependencies]
leaf = { path = "../leaf" }
"#,
        );
        write(&middle.join("src/main.p7"), "fn main() {}");
        write(
            &leaf.join("p7.toml"),
            "[package]\nname = \"leaf\"\nversion = \"0.1.0\"\n",
        );
        write(&leaf.join("src/main.p7"), "fn main() {}");
        write(&leaf.join("src/value.p7"), "pub fn get() -> int { 42 }");

        let project = Project::load(&app).expect("load project graph");
        let error = project
            .compile()
            .expect_err("transitive package should not be visible");
        assert!(error.to_string().contains("Cannot import module"));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn relative_import_from_directory_entry_stays_under_directory() {
        let root = temp_dir("directory-entry");
        write(
            &root.join("p7.toml"),
            r#"
[package]
name = "app"
version = "0.1.0"
entry = "src/net/mod.p7"
"#,
        );
        write(
            &root.join("src/net/mod.p7"),
            "import .socket; fn main() -> int { socket.value() }",
        );
        write(
            &root.join("src/net/socket.p7"),
            "pub fn value() -> int { 42 }",
        );

        let project = Project::load(&root).expect("load project");
        project.compile().expect("compile directory entry");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn artifact_resolution_writes_portable_lock_and_uses_locked_cache() {
        let root = temp_dir("artifact-lock");
        let release = root.join("release");
        let app = root.join("app");
        let cache = root.join("cache");
        let archive = release.join("utility.tar.gz");
        let checksum = write_archive(
            &archive,
            &[
                (
                    "p7.toml",
                    b"[package]\nname = \"utility\"\nversion = \"1.2.3\"\nkind = \"library\"\n",
                ),
                ("src/mod.p7", b"pub fn answer() -> int { 42 }"),
            ],
        );
        let index = release.join("utility.index.toml");
        write_index(&index, "utility", "1.2.3", "", "utility.tar.gz", &checksum);
        write_artifact_app(&app, "utility", &file_url(&index));

        let project = Project::load_for_test(&app, cache.clone(), "aarch64-apple-darwin")
            .expect("resolve artifact");
        project.compile().expect("compile artifact project");
        let lock = project.write_lockfile().expect("write v2 lock");
        let contents = fs::read_to_string(lock).expect("read lock");
        assert!(contents.contains("version = 2"));
        assert!(contents.contains("kind = \"artifact\""));
        assert!(contents.contains("index_sha256 = \""));
        for target in [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
        ] {
            assert!(contents.contains(target), "missing target {target}");
        }
        assert!(contents.contains(&file_url(&archive)));

        write_index(
            &index,
            "utility",
            "9.9.9",
            "",
            "changed.tar.gz",
            &"0".repeat(64),
        );
        fs::remove_file(&archive).expect("remove remote archive");
        let locked = Project::load_for_test(&app, cache.clone(), "x86_64-pc-windows-msvc")
            .expect("locked resolution must not refetch changed index");
        assert_eq!(
            locked
                .packages
                .get("utility")
                .unwrap()
                .manifest
                .package
                .version,
            "1.2.3"
        );
        write(
            &cache.join("packages").join(&checksum).join("src/mod.p7"),
            "pub fn answer() -> int { 0 }",
        );
        let error = Project::load_for_test(&app, cache.clone(), "aarch64-apple-darwin")
            .expect_err("corrupted extracted cache must fail");
        assert!(
            error.contains("Corrupted extracted package cache"),
            "{error}"
        );
        write(
            &cache.join("packages").join(&checksum).join("src/mod.p7"),
            "pub fn answer() -> int { 42 }",
        );
        write(
            &cache.join("archives").join(format!("{checksum}.tar.gz")),
            "bad",
        );
        let error = Project::load_for_test(&app, cache, "aarch64-apple-darwin")
            .expect_err("corrupted archive cache must fail");
        assert!(
            error.contains("Corrupted artifact archive cache"),
            "{error}"
        );
        write_artifact_app(
            &app,
            "utility",
            &file_url(&release.join("replacement.index.toml")),
        );
        let error = Project::load_for_test(&app, root.join("other-cache"), "aarch64-apple-darwin")
            .expect_err("changed dependency URL must conflict with lock");
        assert!(error.contains("but p7.toml specifies"), "{error}");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn artifact_checksum_and_metadata_mismatches_are_clear() {
        let root = temp_dir("artifact-mismatch");
        let release = root.join("release");
        let app = root.join("app");
        let archive = release.join("utility.tar.gz");
        let checksum = write_archive(
            &archive,
            &[
                (
                    "p7.toml",
                    b"[package]\nname = \"utility\"\nversion = \"1.0.0\"\nkind = \"library\"\n",
                ),
                ("src/mod.p7", b"pub fn answer() -> int { 42 }"),
            ],
        );
        let index = release.join("utility.index.toml");
        write_index(
            &index,
            "utility",
            "1.0.0",
            "",
            "utility.tar.gz",
            &"f".repeat(64),
        );
        write_artifact_app(&app, "utility", &file_url(&index));
        let error = Project::load_for_test(
            &app,
            root.join("bad-checksum-cache"),
            "x86_64-unknown-linux-gnu",
        )
        .expect_err("checksum must fail");
        assert!(error.contains("Checksum mismatch"), "{error}");

        write_index(&index, "utility", "2.0.0", "", "utility.tar.gz", &checksum);
        let error = Project::load_for_test(
            &app,
            root.join("bad-metadata-cache"),
            "x86_64-unknown-linux-gnu",
        )
        .expect_err("metadata must fail");
        assert!(error.contains("metadata mismatch"), "{error}");

        write_index(
            &index,
            "utility",
            "1.0.0",
            "[dependencies]\nother = { index = \"https://example.invalid/other.index.toml\" }\n",
            "utility.tar.gz",
            &checksum,
        );
        let error = Project::load_for_test(
            &app,
            root.join("bad-dependency-cache"),
            "x86_64-unknown-linux-gnu",
        )
        .expect_err("dependency metadata must fail");
        assert!(error.contains("dependency sources"), "{error}");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn artifact_archives_reject_traversal_and_links() {
        let root = temp_dir("unsafe-archives");
        for (label, archive_writer, expected) in [
            (
                "traversal",
                write_traversal_archive as fn(&Path) -> String,
                "Unsafe artifact archive path",
            ),
            (
                "link",
                write_link_archive,
                "links and special files are forbidden",
            ),
            ("duplicate", write_duplicate_archive, "duplicate path"),
        ] {
            let release = root.join(label);
            let app = release.join("app");
            let archive = release.join("bad.tar.gz");
            let checksum = archive_writer(&archive);
            let index = release.join("bad.index.toml");
            write_index(&index, "bad", "1.0.0", "", "bad.tar.gz", &checksum);
            write_artifact_app(&app, "bad", &file_url(&index));
            let error =
                Project::load_for_test(&app, release.join("cache"), "x86_64-pc-windows-msvc")
                    .expect_err("unsafe archive must fail");
            assert!(error.contains(expected), "{error}");
        }
        assert!(!root.join("escape").exists());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn artifact_graph_supports_mixed_and_recursive_sources() {
        let root = temp_dir("artifact-graph");
        let release = root.join("release");
        let app = root.join("app");
        let path_dep = root.join("local");
        let leaf_archive = release.join("leaf.tar.gz");
        let leaf_checksum = write_archive(
            &leaf_archive,
            &[
                (
                    "p7.toml",
                    b"[package]\nname = \"leaf\"\nversion = \"1.0.0\"\nkind = \"library\"\n",
                ),
                ("src/mod.p7", b"pub fn value() -> int { 40 }"),
            ],
        );
        let leaf_index = release.join("leaf.index.toml");
        write_index(
            &leaf_index,
            "leaf",
            "1.0.0",
            "",
            "leaf.tar.gz",
            &leaf_checksum,
        );
        let middle_manifest = format!(
            "[package]\nname = \"middle\"\nversion = \"1.0.0\"\nkind = \"library\"\n\n[dependencies]\nleaf = {{ index = {:?} }}\n",
            file_url(&leaf_index)
        );
        let middle_archive = release.join("middle.tar.gz");
        let middle_checksum = write_archive(
            &middle_archive,
            &[
                ("p7.toml", middle_manifest.as_bytes()),
                (
                    "src/mod.p7",
                    b"import leaf; pub fn value() -> int { leaf.value() + 1 }",
                ),
            ],
        );
        let middle_index = release.join("middle.index.toml");
        let index_dependencies = format!(
            "[dependencies]\nleaf = {{ index = {:?} }}\n",
            file_url(&leaf_index)
        );
        write_index(
            &middle_index,
            "middle",
            "1.0.0",
            &index_dependencies,
            "middle.tar.gz",
            &middle_checksum,
        );
        write(
            &path_dep.join("p7.toml"),
            "[package]\nname = \"local\"\nversion = \"1.0.0\"\nkind = \"library\"\n",
        );
        write(&path_dep.join("src/mod.p7"), "pub fn value() -> int { 1 }");
        write(
            &app.join("p7.toml"),
            &format!(
                "[package]\nname = \"app\"\nversion = \"1.0.0\"\n\n[dependencies]\nmiddle = {{ index = {:?} }}\nlocal = {{ path = \"../local\" }}\n",
                file_url(&middle_index)
            ),
        );
        write(
            &app.join("src/main.p7"),
            "import middle; import local; fn main() -> int { middle.value() + local.value() }",
        );

        let project = Project::load_for_test(&app, root.join("cache"), "x86_64-apple-darwin")
            .expect("load mixed recursive graph");
        assert_eq!(project.package_names().count(), 4);
        project.compile().expect("compile mixed recursive graph");
        let lock = project.write_lockfile().expect("write mixed lock");
        let contents = fs::read_to_string(lock).unwrap();
        assert!(contents.contains("kind = \"path\""));
        assert!(contents.contains("kind = \"artifact\""));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn downloaded_packages_reject_path_dependencies() {
        let root = temp_dir("artifact-path-dependency");
        let release = root.join("release");
        let app = root.join("app");
        let archive = release.join("bad.tar.gz");
        let checksum = write_archive(
            &archive,
            &[
                (
                    "p7.toml",
                    b"[package]\nname = \"bad\"\nversion = \"1.0.0\"\nkind = \"library\"\n\n[dependencies]\nescape = { path = \"..\" }\n",
                ),
                ("src/mod.p7", b"pub fn value() -> int { 1 }"),
            ],
        );
        let index = release.join("bad.index.toml");
        write_index(
            &index,
            "bad",
            "1.0.0",
            "[dependencies]\nescape = { path = \"..\" }\n",
            "bad.tar.gz",
            &checksum,
        );
        write_artifact_app(&app, "bad", &file_url(&index));
        let error = Project::load_for_test(&app, root.join("cache"), "aarch64-apple-darwin")
            .expect_err("artifact path dependency must fail");
        assert!(error.contains("cannot use path dependency"), "{error}");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn unsupported_targets_and_url_schemes_are_reported() {
        let root = temp_dir("artifact-target");
        let release = root.join("release");
        let app = root.join("app");
        let archive = release.join("utility.tar.gz");
        let checksum = write_archive(
            &archive,
            &[
                (
                    "p7.toml",
                    b"[package]\nname = \"utility\"\nversion = \"1.0.0\"\nkind = \"library\"\n",
                ),
                ("src/mod.p7", b"pub fn value() -> int { 1 }"),
            ],
        );
        let index = release.join("utility.index.toml");
        write(
            &index,
            &format!(
                "version = 1\n[package]\nname = \"utility\"\nversion = \"1.0.0\"\n[targets.aarch64-apple-darwin]\nurl = \"utility.tar.gz\"\nsha256 = \"{checksum}\"\nformat = \"tar.gz\"\n"
            ),
        );
        write_artifact_app(&app, "utility", &file_url(&index));
        let error = Project::load_for_test(&app, root.join("cache"), "x86_64-unknown-linux-gnu")
            .expect_err("missing target must fail");
        assert!(error.contains("no archive for target"), "{error}");

        write_artifact_app(&app, "utility", "http://example.invalid/index.toml");
        let error = Project::load_for_test(&app, root.join("cache-http"), "aarch64-apple-darwin")
            .expect_err("http must fail");
        assert!(error.contains("expected https or file"), "{error}");

        write_artifact_app(
            &app,
            "utility",
            &file_url(&release.join("missing.index.toml")),
        );
        let error =
            Project::load_for_test(&app, root.join("cache-missing"), "aarch64-apple-darwin")
                .expect_err("missing index must fail");
        assert!(error.contains("Cannot open artifact index"), "{error}");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn version_one_path_lockfiles_upgrade_cleanly() {
        let root = temp_dir("v1-lock");
        write(
            &root.join("p7.toml"),
            "[package]\nname = \"app\"\nversion = \"1.0.0\"\n",
        );
        write(&root.join("src/main.p7"), "fn main() {}");
        write(
            &root.join("p7.lock"),
            "version = 1\n\n[[packages]]\nname = \"app\"\nversion = \"1.0.0\"\nsource = \"path+.\"\nchecksum = \"old\"\ndependencies = []\n",
        );

        let project = Project::load_for_test(&root, root.join("cache"), "aarch64-apple-darwin")
            .expect("v1 path lock remains readable");
        let lock = project.write_lockfile().expect("upgrade lock");
        assert!(fs::read_to_string(lock).unwrap().contains("version = 2"));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn loads_native_extension_from_extracted_artifact() {
        let root = temp_dir("artifact-native");
        let release = root.join("release");
        let app = root.join("app");
        fs::create_dir_all(&release).expect("create release");
        let library_name = format!(
            "{}fixture{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        );
        let library = release.join(&library_name);
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../p7/tests/fixtures/native_extension.rs");
        let output = Command::new("rustc")
            .arg("--edition=2021")
            .arg("--crate-type=cdylib")
            .arg(&fixture)
            .arg("-o")
            .arg(&library)
            .output()
            .expect("run rustc");
        assert!(
            output.status.success(),
            "fixture compilation failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let manifest = format!(
            "[package]\nname = \"nativepkg\"\nversion = \"1.0.0\"\nkind = \"library\"\n\n[native]\nextensions = [\"native/{library_name}\"]\n"
        );
        let library_bytes = fs::read(&library).expect("read native fixture");
        let archive = release.join("nativepkg.tar.gz");
        let checksum = write_archive(
            &archive,
            &[
                ("p7.toml", manifest.as_bytes()),
                ("src/mod.p7", b"pub fn marker() -> int { 1 }"),
                (&format!("native/{library_name}"), &library_bytes),
            ],
        );
        let index = release.join("nativepkg.index.toml");
        write_index(
            &index,
            "nativepkg",
            "1.0.0",
            "",
            "nativepkg.tar.gz",
            &checksum,
        );
        write_artifact_app(&app, "nativepkg", &file_url(&index));
        write(
            &app.join("src/main.p7"),
            "@intrinsic(name=\"dynamic.answer\") fn answer() -> int; fn main() -> int { answer() }",
        );

        let project = Project::load_for_test(&app, root.join("cache"), "x86_64-unknown-linux-gnu")
            .expect("load native artifact");
        project
            .validate_supported_features()
            .expect("validate extracted native path");
        let module = project.compile().expect("compile app");
        let mut runtime = p7::embedding::Runtime::new();
        project
            .load_native_extensions(&mut runtime)
            .expect("load extracted native extension");
        runtime.load_module(module);
        assert!(matches!(
            runtime.call("main", Vec::new()).expect("run app"),
            p7::embedding::CallOutcome::Returned(Some(Data::Int(42)))
        ));

        fs::remove_dir_all(root).ok();
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/p7-cli-tests")
            .join(format!("{label}-{}-{nonce}", std::process::id()))
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture directory");
        }
        fs::write(path, contents).expect("write fixture");
    }

    fn file_url(path: &Path) -> String {
        Url::from_file_path(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
            .expect("file URL")
            .to_string()
    }

    fn write_artifact_app(app: &Path, name: &str, index: &str) {
        write(
            &app.join("p7.toml"),
            &format!(
                "[package]\nname = \"app\"\nversion = \"1.0.0\"\n\n[dependencies]\n{name} = {{ index = {index:?} }}\n"
            ),
        );
        write(
            &app.join("src/main.p7"),
            &format!("import {name}; fn main() -> int {{ {name}.answer() }}"),
        );
    }

    fn write_index(
        path: &Path,
        name: &str,
        version: &str,
        dependencies: &str,
        archive_url: &str,
        checksum: &str,
    ) {
        let mut contents = format!(
            "version = 1\n\n[package]\nname = {name:?}\nversion = {version:?}\n\n{dependencies}"
        );
        for target in [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
        ] {
            contents.push_str(&format!(
                "\n[targets.{target}]\nurl = {archive_url:?}\nsha256 = {checksum:?}\nformat = \"tar.gz\"\n"
            ));
        }
        write(path, &contents);
    }

    fn write_archive(path: &Path, files: &[(&str, &[u8])]) -> String {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create archive directory");
        }
        let file = fs::File::create(path).expect("create archive");
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);
        for (name, contents) in files {
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(contents.len() as u64);
            header.set_cksum();
            builder
                .append_data(&mut header, name, Cursor::new(*contents))
                .expect("append archive file");
        }
        let encoder = builder.into_inner().expect("finish tar");
        encoder.finish().expect("finish gzip");
        sha256_path(path)
    }

    fn write_traversal_archive(path: &Path) -> String {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create archive directory");
        }
        let file = fs::File::create(path).expect("create archive");
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);
        let contents = b"escape";
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(contents.len() as u64);
        header.as_mut_bytes()[..9].copy_from_slice(b"../escape");
        header.set_cksum();
        builder
            .append(&header, Cursor::new(contents))
            .expect("append traversal");
        let encoder = builder.into_inner().expect("finish tar");
        encoder.finish().expect("finish gzip");
        sha256_path(path)
    }

    fn write_link_archive(path: &Path) -> String {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create archive directory");
        }
        let file = fs::File::create(path).expect("create archive");
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Symlink);
        header.set_mode(0o777);
        header.set_size(0);
        header.set_path("link").expect("set link path");
        header.set_link_name("../escape").expect("set link target");
        header.set_cksum();
        builder
            .append(&header, Cursor::new(Vec::<u8>::new()))
            .expect("append link");
        let encoder = builder.into_inner().expect("finish tar");
        encoder.finish().expect("finish gzip");
        sha256_path(path)
    }

    fn write_duplicate_archive(path: &Path) -> String {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create archive directory");
        }
        let file = fs::File::create(path).expect("create archive");
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);
        for contents in [b"first".as_slice(), b"second".as_slice()] {
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(contents.len() as u64);
            header.set_cksum();
            builder
                .append_data(&mut header, "p7.toml", Cursor::new(contents))
                .expect("append duplicate");
        }
        let encoder = builder.into_inner().expect("finish tar");
        encoder.finish().expect("finish gzip");
        sha256_path(path)
    }

    fn sha256_path(path: &Path) -> String {
        format!(
            "{:x}",
            Sha256::digest(fs::read(path).expect("read archive"))
        )
    }
}
