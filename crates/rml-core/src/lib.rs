pub mod dirs {
    use std::path::PathBuf;
    pub fn data_dir() -> PathBuf {
        std::env::var("RML_DATA_DIR")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs_next::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".rustmc-launcher")
            })
    }

    pub fn cache_dir() -> PathBuf {
        data_dir().join("cache")
    }

    pub fn runtimes_dir() -> PathBuf {
        data_dir().join("runtimes")
    }
}
