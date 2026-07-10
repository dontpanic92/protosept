use p7::ModuleProvider;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

const MANIFEST_NAME: &str = "p7.toml";

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub package: PackageManifest,
    #[serde(default)]
    pub dependencies: BTreeMap<String, DependencyManifest>,
    #[serde(default)]
    pub native: NativeManifest,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub kind: PackageKind,
    #[serde(default = "default_source")]
    pub source: PathBuf,
    pub entry: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PackageKind {
    Library,
    #[default]
    Executable,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DependencyManifest {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, Deserialize)]
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
}

#[derive(Debug, Clone)]
pub struct Project {
    root_package: String,
    packages: HashMap<String, Package>,
}

impl Project {
    pub fn load(path: &Path) -> Result<Self, String> {
        let manifest_path = manifest_path(path)?;
        let mut packages = HashMap::new();
        let mut loading = HashSet::new();
        let root_package =
            load_package_recursive(&manifest_path, &mut packages, &mut loading, None)?;
        Ok(Self {
            root_package,
            packages,
        })
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
                let mut dependencies = package
                    .manifest
                    .dependencies
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>();
                dependencies.sort();
                Ok(LockedPackage {
                    name: package.manifest.package.name.clone(),
                    version: package.manifest.package.version.clone(),
                    source: format!(
                        "path+{}",
                        relative_path(project_root, &package.root).to_string_lossy()
                    ),
                    checksum: package_checksum(package)?,
                    dependencies,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        packages.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));
        let lockfile = Lockfile {
            version: 1,
            packages,
        };
        let encoded = toml::to_string_pretty(&lockfile)
            .map_err(|error| format!("Cannot encode lockfile: {error}"))?;
        let path = self.root_package().root.join("p7.lock");
        fs::write(&path, encoded)
            .map_err(|error| format!("Cannot write '{}': {error}", path.display()))?;
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

#[derive(Debug, Serialize)]
struct Lockfile {
    version: u32,
    packages: Vec<LockedPackage>,
}

#[derive(Debug, Serialize)]
struct LockedPackage {
    name: String,
    version: String,
    source: String,
    checksum: String,
    dependencies: Vec<String>,
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
            return Ok(manifest.package.name);
        }

        for (dependency_name, dependency) in &manifest.dependencies {
            validate_package_name(dependency_name)?;
            let dependency_manifest = root.join(&dependency.path).join(MANIFEST_NAME);
            load_package_recursive(
                &dependency_manifest,
                packages,
                loading,
                Some(dependency_name),
            )?;
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

fn package_checksum(package: &Package) -> Result<String, String> {
    let mut files = vec![package.root.join(MANIFEST_NAME)];
    collect_source_files(&package.source_dir, &mut files)?;
    files.sort();

    let mut hasher = Sha256::new();
    for file in files {
        let relative = file
            .strip_prefix(&package.root)
            .map_err(|_| format!("Cannot checksum '{}' outside package root", file.display()))?;
        hasher.update(relative.to_string_lossy().as_bytes());
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
    use p7::interpreter::context::{Context, Data};
    use std::time::{SystemTime, UNIX_EPOCH};

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
        assert!(lock_contents.contains("source = \"path+.\""));
        assert!(lock_contents.contains("source = \"path+../math\""));
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

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("protosept-{label}-{}-{nonce}", std::process::id()))
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture directory");
        }
        fs::write(path, contents).expect("write fixture");
    }
}
