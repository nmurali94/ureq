use crate::url::{Error as ParseError};

use std::error;
use std::fmt::{self, Display};
use std::io;

#[derive(Debug)]
pub enum Error {
    /// There was an error making the request or receiving the response.
    Transport(Transport),
    /// Url Error
    ParseError(ParseError),
}

// Any error that is not a status code error. For instance, DNS name not found,
// connection refused, or malformed response.
#[derive(Debug)]
pub struct Transport {
    kind: ErrorKind,
    message: Option<&'static str>,
    source: Option<Box<dyn error::Error + Send + Sync + 'static>>,
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Transport(err) => {
                write!(f, "{}", err)?;
            }
            Error::ParseError(err) => {
                write!(f, "{}", err)?;
            }
        }
        Ok(())
    }
}

impl Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(message) = &self.message {
            write!(f, ": {}", message)?;
        }
        if let Some(source) = &self.source {
            write!(f, ": {}", source)?;
        }
        Ok(())
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self {
            Error::Transport(Transport {
                source: Some(s), ..
            }) => Some(s.as_ref()),
            _ => None,
        }
    }
}

impl error::Error for Transport {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|s| s.as_ref() as &(dyn error::Error + 'static))
    }
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, message: Option<&'static str>) -> Self {
        Error::Transport(Transport {
            kind,
            message,
            source: None,
        })
    }

    pub(crate) fn src(self, e: impl error::Error + Send + Sync + 'static) -> Self {
        if let Error::Transport(mut oe) = self {
            oe.source = Some(Box::new(e));
            Error::Transport(oe)
        } else {
            self
        }
    }

    /// The type of this error.
    ///
    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::Transport(Transport { kind: k, .. }) => *k,
            Error::ParseError(_) => ErrorKind::InvalidUrl,
        }
    }
}

/// One of the types of error the can occur when processing a Request.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ErrorKind {
    /// The url could not be understood.
    InvalidUrl,
    /// The url scheme could not be understood.
    UnknownScheme,
    /// DNS lookup failed.
    Dns,
    /// Connection to server failed.
    ConnectionFailed,
    /// Too many redirects.
    TooManyRedirects,
    /// A status line we don't understand `HTTP/1.1 200 OK`.
    BadStatus,
    /// A header line that couldn't be parsed.
    BadHeader,
    /// Some unspecified `std::io::Error`.
    Io,
    /// Proxy information was not properly formatted
    InvalidProxyUrl,
    /// Proxy could not connect
    ProxyConnect,
    /// Incorrect credentials for proxy
    ProxyUnauthorized,
    /// HTTP status code indicating an error (e.g. 4xx, 5xx)
    /// Read the inner response body for details and to return
    /// the connection to the pool.
    HTTP,
}

impl ErrorKind {
    #[allow(clippy::wrong_self_convention)]
    #[allow(clippy::new_ret_no_self)]
    pub(crate) fn new(self) -> Error {
        Error::new(self, None)
    }

    pub(crate) fn msg(self, s: &'static str) -> Error {
        Error::new(self, Some(s))
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        ErrorKind::Io.new().src(err)
    }
}

impl From<Transport> for Error {
    fn from(err: Transport) -> Error {
        Error::Transport(err)
    }
}

impl From<ParseError> for Error {
    fn from(err: ParseError) -> Self {
        ErrorKind::InvalidUrl.msg("Failed to parse URL")
            .src(err)
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ErrorKind::InvalidUrl => write!(f, "Bad URL"),
            ErrorKind::UnknownScheme => write!(f, "Unknown Scheme"),
            ErrorKind::Dns => write!(f, "Dns Failed"),
            ErrorKind::ConnectionFailed => write!(f, "Connection Failed"),
            ErrorKind::TooManyRedirects => write!(f, "Too Many Redirects"),
            ErrorKind::BadStatus => write!(f, "Bad Status"),
            ErrorKind::BadHeader => write!(f, "Bad Header"),
            ErrorKind::Io => write!(f, "Network Error"),
            ErrorKind::InvalidProxyUrl => write!(f, "Malformed proxy"),
            ErrorKind::ProxyConnect => write!(f, "Proxy failed to connect"),
            ErrorKind::ProxyUnauthorized => write!(f, "Provided proxy credentials are incorrect"),
            ErrorKind::HTTP => write!(f, "HTTP status error"),
        }
    }
}
