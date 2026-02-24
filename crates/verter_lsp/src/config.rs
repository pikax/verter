use std::path::{Path, PathBuf};

/// Discovers tsconfig.json files in a workspace and maps directories to their configs.
///
/// Ports the logic from `VerterManager.findTsServices()` in the TypeScript language server.
pub struct TsConfigDiscovery {
    /// Map from directory pattern (e.g., "/project/src/**") to tsconfig path.
    configs: Vec<TsConfigEntry>,
}

/// A discovered tsconfig.json and its coverage pattern.
#[derive(Debug, Clone)]
pub struct TsConfigEntry {
    /// Absolute path to the tsconfig.json file.
    pub config_path: PathBuf,
    /// Glob pattern for files covered by this tsconfig.
    pub pattern: String,
}

impl TsConfigDiscovery {
    pub fn new() -> Self {
        Self {
            configs: Vec::new(),
        }
    }

    /// Discover all tsconfig.json files under the given workspace root.
    ///
    /// Searches recursively, excluding `node_modules` and dot-directories.
    pub fn discover(&mut self, root: &Path) {
        let pattern = root.join("**/tsconfig.json").to_string_lossy().to_string();
        let pattern = pattern.replace('\\', "/");

        match glob::glob(&pattern) {
            Ok(paths) => {
                for entry in paths.flatten() {
                    // Skip node_modules
                    if entry.components().any(|c| c.as_os_str() == "node_modules") {
                        continue;
                    }
                    // Skip dot-directories
                    if entry
                        .components()
                        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
                    {
                        continue;
                    }

                    if let Some(dir) = entry.parent() {
                        let coverage = format!("{}/**", dir.to_string_lossy().replace('\\', "/"));
                        self.configs.push(TsConfigEntry {
                            config_path: entry,
                            pattern: coverage,
                        });
                    }
                }
            }
            Err(e) => {
                tracing::warn!("failed to glob for tsconfig.json: {}", e);
            }
        }
    }

    /// Find the tsconfig.json that covers a given file path.
    ///
    /// Returns the most specific (longest directory prefix) match.
    pub fn find_config_for(&self, file_path: &Path) -> Option<&TsConfigEntry> {
        let file_str = file_path.to_string_lossy().replace('\\', "/");
        let mut best: Option<&TsConfigEntry> = None;
        let mut best_prefix_len = 0;

        for entry in &self.configs {
            // Extract the directory prefix from the pattern (everything before /**)
            let prefix = entry.pattern.trim_end_matches("/**");
            if file_str.starts_with(prefix) && prefix.len() > best_prefix_len {
                best_prefix_len = prefix.len();
                best = Some(entry);
            }
        }

        best
    }

    /// Get all discovered tsconfig entries.
    pub fn configs(&self) -> &[TsConfigEntry] {
        &self.configs
    }
}

impl Default for TsConfigDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_config_most_specific() {
        let mut discovery = TsConfigDiscovery::new();
        discovery.configs.push(TsConfigEntry {
            config_path: PathBuf::from("/project/tsconfig.json"),
            pattern: "/project/**".into(),
        });
        discovery.configs.push(TsConfigEntry {
            config_path: PathBuf::from("/project/packages/app/tsconfig.json"),
            pattern: "/project/packages/app/**".into(),
        });

        // File in nested package should match the more specific config
        let result = discovery.find_config_for(Path::new("/project/packages/app/src/main.ts"));
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().config_path,
            PathBuf::from("/project/packages/app/tsconfig.json")
        );
    }

    #[test]
    fn test_find_config_fallback_to_root() {
        let mut discovery = TsConfigDiscovery::new();
        discovery.configs.push(TsConfigEntry {
            config_path: PathBuf::from("/project/tsconfig.json"),
            pattern: "/project/**".into(),
        });

        // File outside specific packages should match root config
        let result = discovery.find_config_for(Path::new("/project/src/utils.ts"));
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().config_path,
            PathBuf::from("/project/tsconfig.json")
        );
    }

    #[test]
    fn test_find_config_no_match() {
        let discovery = TsConfigDiscovery::new();

        let result = discovery.find_config_for(Path::new("/other/project/src/main.ts"));
        assert!(result.is_none());
    }
}
