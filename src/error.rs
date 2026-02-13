#[derive(Debug, thiserror::Error)]
pub enum CollectorError {
    #[error("failed to read {path}: {source}")]
    FileRead {
        path: String,
        source: std::io::Error,
    },

    #[error("failed to parse {field} in {path}: {raw}")]
    Parse {
        path: String,
        field: String,
        raw: String,
    },
}
