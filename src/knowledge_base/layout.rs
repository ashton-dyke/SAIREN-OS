//! Directory layout helpers for the knowledge base

use crate::types::KnowledgeBaseConfig;
use std::io;
use std::path::PathBuf;

impl KnowledgeBaseConfig {
    /// Root directory for the field
    pub fn field_dir(&self) -> PathBuf {
        self.root.join(&self.field)
    }

    /// Path to field-level geology file
    pub fn geology_path(&self) -> PathBuf {
        self.field_dir().join("geology.toml")
    }

    /// Root directory for a specific well
    pub fn well_dir(&self, well: &str) -> PathBuf {
        self.field_dir().join("wells").join(well)
    }

    /// Pre-spud directory for a specific well
    pub fn pre_spud_dir(&self, well: &str) -> PathBuf {
        self.well_dir(well).join("pre-spud")
    }

    /// Pre-spud prognosis file for a specific well
    pub fn pre_spud_path(&self, well: &str) -> PathBuf {
        self.pre_spud_dir(well).join("prognosis.toml")
    }

    /// Mid-well snapshot directory for the current well
    pub fn mid_well_dir(&self) -> PathBuf {
        self.well_dir(&self.well).join("mid-well")
    }

    /// Post-well directory for a specific well
    pub fn post_well_dir(&self, well: &str) -> PathBuf {
        self.well_dir(well).join("post-well")
    }

    /// Create all required directories for the current well
    pub fn ensure_dirs(&self) -> io::Result<()> {
        std::fs::create_dir_all(self.field_dir())?;
        std::fs::create_dir_all(self.pre_spud_dir(&self.well))?;
        std::fs::create_dir_all(self.mid_well_dir())?;
        std::fs::create_dir_all(self.post_well_dir(&self.well))?;
        Ok(())
    }

    /// List all sibling wells (all wells in the field except the current one)
    pub fn list_sibling_wells(&self) -> io::Result<Vec<String>> {
        let wells_dir = self.field_dir().join("wells");
        if !wells_dir.exists() {
            return Ok(Vec::new());
        }

        let mut siblings = Vec::new();
        for entry in std::fs::read_dir(&wells_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    if name != self.well {
                        siblings.push(name.to_string());
                    }
                }
            }
        }
        siblings.sort();
        Ok(siblings)
    }

    /// List all post-well performance files for a specific well
    pub fn list_post_well_performance(&self, well: &str) -> io::Result<Vec<PathBuf>> {
        let dir = self.post_well_dir(well);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut files = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("performance_") && (name.ends_with(".toml") || name.ends_with(".toml.zst")) {
                    files.push(path);
                }
            }
        }
        files.sort();
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(dir: &std::path::Path) -> KnowledgeBaseConfig {
        KnowledgeBaseConfig {
            root: dir.to_path_buf(),
            field: "TestField".to_string(),
            well: "Well-A".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_path_helpers() {
        let dir = std::path::Path::new("/tmp/kb-test");
        let cfg = test_config(dir);

        assert_eq!(cfg.field_dir(), dir.join("TestField"));
        assert_eq!(cfg.geology_path(), dir.join("TestField/geology.toml"));
        assert_eq!(cfg.well_dir("Well-A"), dir.join("TestField/wells/Well-A"));
        assert_eq!(cfg.pre_spud_dir("Well-A"), dir.join("TestField/wells/Well-A/pre-spud"));
        assert_eq!(cfg.mid_well_dir(), dir.join("TestField/wells/Well-A/mid-well"));
        assert_eq!(cfg.post_well_dir("Well-B"), dir.join("TestField/wells/Well-B/post-well"));
    }

    #[test]
    fn test_ensure_dirs_and_list_siblings() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg = test_config(tmp.path());
        cfg.ensure_dirs().expect("ensure_dirs");

        // Create a sibling well directory
        let sibling_dir = cfg.well_dir("Well-B");
        std::fs::create_dir_all(&sibling_dir).expect("mkdir sibling");

        let siblings = cfg.list_sibling_wells().expect("list siblings");
        assert_eq!(siblings, vec!["Well-B"]);
    }

    #[test]
    fn test_list_post_well_performance() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg = test_config(tmp.path());
        let post_dir = cfg.post_well_dir("Well-B");
        std::fs::create_dir_all(&post_dir).expect("mkdir");

        // Create some performance files
        std::fs::write(post_dir.join("performance_Nordland.toml"), "").expect("write");
        std::fs::write(post_dir.join("performance_Hugin.toml.zst"), "").expect("write");
        std::fs::write(post_dir.join("summary.toml"), "").expect("write"); // should be excluded

        let files = cfg.list_post_well_performance("Well-B").expect("list");
        assert_eq!(files.len(), 2);
    }
}
