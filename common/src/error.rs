use thiserror::Error;

#[derive(Debug, Error)]
pub enum RouteWeaverError {
    #[error("The author should be fixing this")]
    IDontWannaRationalizeThisRightNow,
    #[error("Suspicious remote behavior")]
    SuspiciousRemoteBehavior,
    #[error("Handshake in established tunnel")]
    HandshakeInEstablishedTunnel,
    #[error("Handshake ordering is off")]
    HandshakeOrderingOff,
    #[error("Operation unsupported for this type of transport")]
    UnsupportedOperationRequestedOnTransport,
    #[error("Failed to parse address")]
    AddressFailedToParse,
    #[error("Failed to parse key")]
    KeyFailedToParse,
    #[error("Failed to parse packet")]
    PacketManagingFailure,
    #[error("Failed to process message")]
    UnencryptedMessageProcessingError,
    #[error("Postcard related error: {0}")]
    PostcardRelatedError(#[from] postcard::Error),
    #[error("Snow related error: {0}")]
    SnowRelatedError(#[from] snow::Error),
    #[error("Unknown error: {0}")]
    Custom(#[from] std::io::Error),
}
