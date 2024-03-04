use thiserror::Error;

#[derive(Debug, Error)]
pub enum RouteWeaverError {
    #[error("Operation unsupported for this type of transport")]
    UnsupportedOperationRequestedOnTransport,

    #[error("Failed to parse address")]
    AddressFailedToParse,
    #[error("Failed to parse key")]
    KeyFailedToParse,
    #[error("Lz4 Compression related error: {0}")]
    Lz4CompressionRelatedError(#[from] lz4_flex::block::CompressError),
    #[error("Lz4 Decompression related error: {0}")]
    Lz4DecompressionRelatedError(#[from] lz4_flex::block::DecompressError),
    #[error("Bincode related error: {0}")]
    BincodeRelatedError(#[from] bincode::Error),
    #[error("Snow related error: {0}")]
    SnowRelatedError(#[from] snow::Error),
    #[error("Unknown error: {0}")]
    Custom(#[from] std::io::Error),
}
