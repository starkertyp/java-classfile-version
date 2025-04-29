use glob::glob;
use std::{
    env::args,
    fmt::Display,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use thiserror::Error;
use tracing::{debug, info, instrument, trace, warn};
use zip::result::ZipError;

#[derive(Debug, PartialEq, PartialOrd, Clone)]
struct JavaClass(pub u8);

impl Display for JavaClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (Java {})", self.0, self.0 - 44)
    }
}

const MAGIC_HEADER: [u8; 4] = [202, 254, 186, 190]; // CAFEBABE

#[derive(Error, Debug)]
enum JavaClassError {
    #[error("Failed to read bytes from file")]
    Read(#[from] io::Error),
    #[error("Should have got at least 8 bytes, got {0}")]
    InsufficientBytes(usize),
    #[error("Not a java class")]
    NotAClassFile,
}

impl TryFrom<File> for JavaClass {
    type Error = JavaClassError;

    fn try_from(mut f: File) -> Result<Self, Self::Error> {
        let mut buffer = [0; 8];

        let read_bytes = f.read(&mut buffer)?;
        if read_bytes != 8 {
            return Err(JavaClassError::InsufficientBytes(read_bytes));
        }

        if !buffer.starts_with(&MAGIC_HEADER) {
            return Err(JavaClassError::NotAClassFile);
        }

        let version = buffer[7];

        Ok(JavaClass(version))
    }
}

#[derive(Error, Debug)]
enum ExtractedJarError {
    #[error("I/O Error")]
    IO(#[from] io::Error),
    #[error("Failed to read jar as zip file")]
    Zip(#[from] ZipError),
    #[error("Failed to read extracted files")]
    GlobPattern(#[from] glob::PatternError),
    #[error("Glob error")]
    Glob(#[from] glob::GlobError),
}

struct ExtractedJar {
    rootdir: TempDir,
    classfiles: Vec<PathBuf>,
}

impl TryFrom<String> for ExtractedJar {
    type Error = ExtractedJarError;

    #[instrument]
    fn try_from(file: String) -> Result<Self, Self::Error> {
        let file = File::open(file)?;
        trace!("Reading archive at {file:?}");
        let mut archive = zip::ZipArchive::new(file)?;
        trace!("Got archive {archive:?}");
        // TODO need to clean this up afterwards
        let targetdir = tempfile::TempDir::with_prefix("java-classfile-version")?;
        trace!("Created temporary directory {targetdir:?}");
        let path = targetdir.path();
        debug!("Trying to extract the JAR");
        archive.extract(path)?;
        debug!("Extraction successful");
        let mut classfiles = Vec::new();
        for file in glob(&format!("{}/**/*.class", path.display()))? {
            match file {
                Ok(file) => {
                    if !file.starts_with("META") {
                        debug!("Found relevant file {file:?} in JAR");
                        classfiles.push(file);
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(Self {
            rootdir: targetdir,
            classfiles,
        })
    }
}

#[instrument(skip_all)]
fn handle_class<P: AsRef<Path>>(file: P) -> Result<JavaClass, JavaClassError> {
    let file = File::open(file).unwrap();
    debug!("Read {file:?}");
    let class = JavaClass::try_from(file).unwrap();
    Ok(class)
}

fn main() {
    tracing_subscriber::fmt::init();
    let args: Vec<_> = args().skip(1).collect();

    for file in args {
        if file.ends_with(".jar") {
            info!("Handling JAR file {file}");
            let extraced = ExtractedJar::try_from(file).unwrap();
            let mut largest: Option<JavaClass> = None;
            for file in extraced.classfiles {
                let p = file.to_string_lossy();
                debug!("Reading from {p}");
                let class = handle_class(file).unwrap();
                match largest {
                    Some(ref prev) => {
                        if *prev < class {
                            largest = Some(class)
                        }
                    }
                    None => largest = Some(class),
                }
            }
            if let Some(class) = largest {
                info!("Largest class version is {class}");
            } else {
                warn!("Failed to find a single class");
            }
        } else if file.ends_with(".class") {
            info!("Reading from {file}");
            let class = handle_class(file).unwrap();
            info!("Class version is {}", class);
        }
    }
}
