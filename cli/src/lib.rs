pub type Result<T> = std::result::Result<T, CliError>;

#[derive(Debug)]
pub enum CliError {
    CliError(String, Box<CliError>),
    ReqwestError(String, reqwest::Error),
    GenericError(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            CliError::CliError(ref message, ref source) => {
                write!(f, "{message}: {source}")
            }
            CliError::ReqwestError(ref message, ref source) => {
                write!(f, "{message}: {source}")
            }
            CliError::GenericError(ref message) => write!(f, "{}", message),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::CliError(.., ref source) => Some(source),
            Self::ReqwestError(.., ref source) => Some(source),
            Self::GenericError(_) => None,
            // Self::DeserializeError { ref source, .. } => Some(source),
            // Self::ValidationError(_) => None,
        }
    }
}

impl From<(String, reqwest::Error)> for CliError {
    fn from(value: (String, reqwest::Error)) -> Self {
        CliError::ReqwestError(value.0, value.1)
    }
}

impl From<(&str, reqwest::Error)> for CliError {
    fn from(value: (&str, reqwest::Error)) -> Self {
        CliError::ReqwestError(value.0.to_string(), value.1)
    }
}

impl From<reqwest::Error> for CliError {
    fn from(value: reqwest::Error) -> Self {
        CliError::ReqwestError("".into(), value)
    }
}

impl From<String> for CliError {
    fn from(value: String) -> Self {
        CliError::GenericError(value)
    }
}

// impl From<(String, std::io::Error)> for CliError {
//     fn from(value: (String, std::io::Error)) -> Self {
//         ParseError::IOError {
//             message: value.0,
//             source: value.1,
//         }
//     }
// }
