use std::str::FromStr;



/// A value that contains one of the ways to authenticate to Assemblyline
pub enum Authentication {
    /// Authenticate with a password
    Password{
        /// The name of the user account connecting
        username: String,
        /// The password of the user connecting
        password: String
    },
    /// Authenticate with an api key
    ApiKey{
        /// The name of the user account connecting
        username: String,
        /// The API key of the user connecting
        key: String
    },
    /// Authenticate with an oauth token
    OAuth{
        /// Oauth provider
        provider: String,
        /// Oauth token
        token: String
    }
}

/// sha256 hash of a file
pub struct Sha256 {
    hex: String
}

impl std::fmt::Display for Sha256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.hex)
    }
}

impl std::ops::Deref for Sha256 {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.hex
    }
}

impl FromStr for Sha256 {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.trim().to_ascii_lowercase();
        if hex.len() != 64 || !hex.chars().all(|c|c.is_ascii_hexdigit()) {
            return Err(Error::InvalidSha256)
        }
        return Ok(Sha256{ hex })
    }
}

/// Short name for serde json's basic map type
pub type JsonMap = serde_json::Map<String, serde_json::Value>;

/// Set of possible errors returned by client
pub enum Error {
    /// An error produced by the client's communication with the server
    Client{
        /// A message describing the error
        message: String,
        /// HTTP status code associated
        status: u32,
        /// Server's API version if available
        api_version: Option<String>,
        /// Server's response details if available
        api_response: Option<String>
    },
    /// An error that occured during a failed communication with the server
    TransportError(String),
    /// An invalid HTTP header name or value was provided
    InvalidHeader,
    /// The server's response was truncated, corrupted, or malformed
    MalformedResponse,
    /// A string could not be converted into a sha256
    InvalidSha256,
}

impl Error {
    pub (crate) fn client_error(message: String, status: u32) -> Self {
        return Error::Client { message, status, api_response: None, api_version: None }
    }
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        if let Some(code) = value.status() {
            Error::client_error(value.to_string(), code.as_u16() as u32)
        } else {
            Error::TransportError(value.to_string())
        }
    }
}

impl From<reqwest::header::InvalidHeaderName> for Error {
    fn from(_value: reqwest::header::InvalidHeaderName) -> Self {
        Self::InvalidHeader
    }
}

impl From<reqwest::header::InvalidHeaderValue> for Error {
    fn from(_value: reqwest::header::InvalidHeaderValue) -> Self {
        Self::InvalidHeader
    }
}