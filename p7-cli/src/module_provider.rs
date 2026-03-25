use p7::ModuleProvider;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Module provider that resolves modules from the filesystem.
///
/// Resolution order:
/// 1. `std.*` modules are resolved from the std library directory (relative to the binary)
/// 2. Other modules are resolved relative to the script's directory
#[derive(Clone)]
pub struct FileSystemModuleProvider {
    /// Base directory for user module resolution (script's directory)
    script_dir: PathBuf,
    /// Directory containing the std library (relative to binary)
    std_dir: PathBuf,
}

impl FileSystemModuleProvider {
    pub fn new(script_path: &Path) -> Self {
        let script_dir = script_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        let std_dir = Self::find_std_dir();

        FileSystemModuleProvider {
            script_dir,
            std_dir,
        }
    }

    /// Find the std library directory relative to the binary location.
    /// Looks for a `std` directory in:
    /// 1. Same directory as the binary
    /// 2. Parent directory of the binary (for development: target/debug/../std)
    fn find_std_dir() -> PathBuf {
        if let Ok(exe_path) = env::current_exe()
            && let Some(exe_dir) = exe_path.parent() {
                // Check same directory as binary
                let std_in_exe_dir = exe_dir.join("std");
                if std_in_exe_dir.is_dir() {
                    return std_in_exe_dir;
                }

                // Check parent directory (for development builds)
                if let Some(parent) = exe_dir.parent() {
                    let std_in_parent = parent.join("std");
                    if std_in_parent.is_dir() {
                        return std_in_parent;
                    }

                    // Check grandparent (target/debug/../../std)
                    if let Some(grandparent) = parent.parent() {
                        let std_in_grandparent = grandparent.join("std");
                        if std_in_grandparent.is_dir() {
                            return std_in_grandparent;
                        }
                    }
                }
            }

        // Fallback to current directory
        PathBuf::from("std")
    }

    /// Convert a dotted module path to a file path.
    /// e.g., "foo.bar" -> "foo/bar.p7"
    fn module_path_to_file_path(module_path: &str) -> PathBuf {
        let parts: Vec<&str> = module_path.split('.').collect();
        let mut path = PathBuf::new();
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                path.push(format!("{}.p7", part));
            } else {
                path.push(part);
            }
        }
        path
    }

    fn load_from_directory(&self, base_dir: &Path, module_path: &str) -> Option<String> {
        let file_path = base_dir.join(Self::module_path_to_file_path(module_path));
        fs::read_to_string(&file_path).ok()
    }
}

impl ModuleProvider for FileSystemModuleProvider {
    fn load_module(&self, module_path: &str) -> Option<String> {
        // Check if it's a std module
        if let Some(relative_path) = module_path.strip_prefix("std.") {
            // Strip "std." prefix and look in std directory
            // Remove "std."
            return self.load_from_directory(&self.std_dir, relative_path);
        }

        if module_path == "std" {
            // Load std/mod.p7 or std.p7 if someone imports just "std"
            let mod_file = self.std_dir.join("mod.p7");
            if mod_file.is_file() {
                return fs::read_to_string(&mod_file).ok();
            }
        }

        // For non-std modules, resolve relative to script directory
        self.load_from_directory(&self.script_dir, module_path)
    }

    fn clone_boxed(&self) -> Box<dyn ModuleProvider> {
        Box::new(self.clone())
    }
}
