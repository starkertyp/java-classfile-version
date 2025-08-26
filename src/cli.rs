use std::sync::Mutex;

use clap::{arg, command, parser::MatchesError, value_parser};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CliError {
    #[error("Failed to parse commandline arguments")]
    Parse(#[from] MatchesError),
    #[error("Did not find any valid paths")]
    NoPaths,
}

#[derive(Debug)]
pub struct Cli {
    pub files: Vec<String>,
    pub max: Option<u16>,
}

pub static LOG_LEVEL: Mutex<u8> = Mutex::new(0);

impl Cli {
    pub fn new() -> Result<Self, CliError> {
        let matches = command!()
            .arg(
                arg!(-m --max <MAXIMUM> "maximum version that is supported by your use case. A version higher than that will result in an exit code > 0")
                    .required(false)
                    .value_parser(value_parser!(u16))
            )
            .arg(
                arg!(<path> ... "files to read")
                    .trailing_var_arg(true)
                    .required(true)
                    .value_parser(value_parser!(String)),
            )
            .arg(
                arg!(-v --verbose ... "verbose logging. can be set multiple times")
)
            .get_matches();

        let paths = matches.try_get_many::<String>("path")?;
        let max = matches.try_get_one::<u16>("max")?;

        if let Some(paths) = paths {
            let paths: Vec<_> = paths.map(|path| path.to_owned()).collect();
            let loglevel = matches.try_get_one::<u8>("verbose")?;
            if let Some(loglevel) = loglevel {
                // this should be safe?
                let mut global_loglevel = LOG_LEVEL.lock().unwrap();
                *global_loglevel = *loglevel;
            }

            Ok(Self {
                files: paths,
                max: max.copied(),
            })
        } else {
            Err(CliError::NoPaths)
        }
    }
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
        println!($($arg)*);
    }};
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {{
        let __loglevel = $crate::cli::LOG_LEVEL.lock().unwrap();
        if *__loglevel >= 1 {
            println!($($arg)*);
        }
    }};
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {{
        let __loglevel = $crate::cli::LOG_LEVEL.lock().unwrap();
        if *__loglevel >= 2 {
            println!($($arg)*);
        }
    }};
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
        eprintln!($($arg)*);
    }};
}
