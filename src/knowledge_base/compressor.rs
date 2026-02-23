//! Transparent zstd compression for TOML files in the knowledge base

use serde::de::DeserializeOwned;
use std::io;
use std::path::Path;

/// Zstd compression level (matches fleet convention)
const ZSTD_LEVEL: i32 = 3;

/// Read a TOML file with transparent zstd decompression.
///
/// Handles both `.toml` (plain) and `.toml.zst` (compressed) files.
pub fn read_toml<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let content = if path.extension().and_then(|e| e.to_str()) == Some("zst") {
        decompress_to_string(path)?
    } else {
        std::fs::read_to_string(path)?
    };

    toml::from_str(&content).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("TOML parse error in {}: {}", path.display(), e),
        )
    })
}

/// Decompress a `.toml.zst` file and return its content as a string.
pub fn decompress_to_string(path: &Path) -> io::Result<String> {
    let compressed = std::fs::read(path)?;
    let decompressed = zstd::decode_all(compressed.as_slice())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("zstd decode error: {}", e)))?;
    String::from_utf8(decompressed)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("UTF-8 error: {}", e)))
}

/// Compress a plain TOML file in-place: creates `.toml.zst` and deletes original.
pub fn compress_file(path: &Path) -> io::Result<()> {
    let content = std::fs::read(path)?;
    let compressed = zstd::encode_all(content.as_slice(), ZSTD_LEVEL)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("zstd encode error: {}", e)))?;

    let mut zst_path = path.as_os_str().to_owned();
    zst_path.push(".zst");
    let zst_path = std::path::PathBuf::from(zst_path);

    std::fs::write(&zst_path, compressed)?;
    std::fs::remove_file(path)?;
    Ok(())
}

/// Write a value as TOML to a file
pub fn write_toml<T: serde::Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let content = toml::to_string_pretty(value).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("TOML serialize error: {}", e))
    })?;
    std::fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestData {
        name: String,
        value: f64,
    }

    #[test]
    fn test_roundtrip_plain_toml() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test.toml");

        let data = TestData { name: "hello".to_string(), value: 42.0 };
        write_toml(&path, &data).expect("write");

        let loaded: TestData = read_toml(&path).expect("read");
        assert_eq!(loaded, data);
    }

    #[test]
    fn test_roundtrip_compressed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test.toml");

        let data = TestData { name: "compressed".to_string(), value: 99.9 };
        write_toml(&path, &data).expect("write");

        // Compress
        compress_file(&path).expect("compress");
        assert!(!path.exists(), "original should be deleted");

        let zst_path = tmp.path().join("test.toml.zst");
        assert!(zst_path.exists(), "compressed file should exist");

        // Read compressed
        let loaded: TestData = read_toml(&zst_path).expect("read compressed");
        assert_eq!(loaded, data);
    }

    #[test]
    fn test_decompress_to_string() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test.toml");

        std::fs::write(&path, "name = \"test\"\nvalue = 1.0\n").expect("write");
        compress_file(&path).expect("compress");

        let zst_path = tmp.path().join("test.toml.zst");
        let content = decompress_to_string(&zst_path).expect("decompress");
        assert!(content.contains("name = \"test\""));
    }
}
