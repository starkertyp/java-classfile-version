mod cli;

use anyhow::bail;
use cli::Cli;
use std::{
    fmt::Display,
    fs::{create_dir_all, File},
    io::{self, Read, Seek},
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use thiserror::Error;
use zip::{result::ZipError, ZipArchive};

#[derive(Debug, PartialEq, PartialOrd, Clone)]
struct JavaClass(pub u8);

impl Display for JavaClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (Java {})", self.0, self.0 - 44)
    }
}

const MAGIC_CLASS_HEADER: [u8; 4] = [202, 254, 186, 190]; // CAFEBABE
const MAGIC_ZIP_HEADER: [u8; 4] = [80, 75, 3, 4];

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

        if !buffer.starts_with(&MAGIC_CLASS_HEADER) {
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
    #[error("Not a Jar file")]
    NotAJar,
    #[error("Should have got at least 4 bytes, got {0}")]
    InsufficientBytes(usize),
}

#[allow(dead_code)]
struct ExtractedJar {
    rootdir: TempDir,
    classfiles: Vec<PathBuf>,
}

impl TryFrom<String> for ExtractedJar {
    type Error = ExtractedJarError;

    fn try_from(file: String) -> Result<Self, Self::Error> {
        let mut file = File::open(file)?;
        trace!("Reading archive at {file:?}");
        let mut buffer = [0; 4];

        let read_bytes = file.read(&mut buffer)?;
        if read_bytes != 4 {
            return Err(ExtractedJarError::InsufficientBytes(read_bytes));
        }
        if !buffer.starts_with(&MAGIC_ZIP_HEADER) {
            return Err(ExtractedJarError::NotAJar);
        }

        let mut archive = zip::ZipArchive::new(file)?;
        trace!("Got archive {archive:?}");
        let targetdir = tempfile::TempDir::with_prefix("java-classfile-version")?;
        trace!("Created temporary directory {targetdir:?}");
        let path = targetdir.path();
        debug!("Trying to extract the JAR");
        let classfiles = get_class_files_in_jar(&archive);
        let mut out_classfiles = Vec::new();
        debug!("classfiles in jar: {classfiles:?}");
        // NOTE: This can't be done in parallel with rayon as the archive can't be borrowed as mutable in that case
        // RwLock doesn't help, can't get a `mut` from `read()` and calling `write()` would lock, defeating the parallel approach completely
        for file in classfiles {
            debug!("Trying to extract {file}");
            trace!("Trying to get a file for {file}");
            let mut file = archive.by_name(&file)?;
            trace!("Got something");

            // realistically this should always be the case
            if let Some(enclosed_name) = file.enclosed_name() {
                trace!("Got name {}", enclosed_name.display());
                let out_path = path.join(enclosed_name);
                trace!("out path: {}", out_path.display());
                // this should,
                let parent = (&out_path).parent();
                if let Some(parent) = parent {
                    create_dir_all(parent)?;
                }
                let mut out_file = File::create(out_path.clone())?;
                std::io::copy(&mut file, &mut out_file)?;
                out_classfiles.push(out_path);
            }
        }

        Ok(Self {
            rootdir: targetdir,
            classfiles: out_classfiles,
        })
    }
}

/// Searches for all .class files outside of a META-INF directory.
///
/// This mostly exists so that the borrow for this drops after this is done,
/// or the archive.by_name later on complains about multiple borrows existing
fn get_class_files_in_jar<T: Read + Seek>(jar: &ZipArchive<T>) -> Vec<String> {
    jar.file_names()
        .filter(|name| name.ends_with(".class"))
        .filter(|name| !name.starts_with("META-INF"))
        .map(|name| name.to_owned())
        .collect()
}

fn handle_class<P: AsRef<Path>>(file: P) -> Result<JavaClass, JavaClassError> {
    let file = File::open(file)?;
    debug!("Read {file:?}");
    let class = JavaClass::try_from(file)?;
    Ok(class)
}

#[derive(Error, Debug)]
pub enum ProgramError {
    #[error("Found a class with version {too_high:?}, which is higher than the given maximum of {max:?}")]
    TooHigh { too_high: u8, max: u8 },
}

fn handle_too_high(class: &JavaClass, max: &Option<u8>, too_high: &mut Option<u8>) {
    if let Some(max) = max {
        trace!("max is set; checking");
        if class.0 > *max {
            trace!("class version {class:?} is higher than {max}!");
            *too_high = Some(class.0);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = Cli::new()?;
    trace!("{args:?}");

    let max = args.max;
    let mut too_high: Option<u8> = None;

    for file in args.files {
        if file.ends_with(".jar") {
            log!("Handling JAR file {file}");
            let extracted = ExtractedJar::try_from(file)?;
            let mut largest: Option<JavaClass> = None;
            for file in extracted.classfiles {
                let p = file.to_string_lossy();
                debug!("Reading from {p}");
                let class = handle_class(file)?;
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
                log!("Largest class version is {class}");
                handle_too_high(&class, &max, &mut too_high);
            } else {
                warn!("Failed to find a single class");
            }
        } else if file.ends_with(".class") {
            log!("Reading from {file}");
            let class = handle_class(file)?;
            log!("Class version is {}", class);
            handle_too_high(&class, &max, &mut too_high);
        } else {
            // no idea what this is, guess
            match handle_class(&file) {
                Ok(class) => {
                    log!("Class version is {}", class);
                    handle_too_high(&class, &max, &mut too_high);
                }
                Err(JavaClassError::NotAClassFile) => {
                    debug!("Not a class file, trying for a jar file");
                    let extracted = ExtractedJar::try_from(file);
                    match extracted {
                        Err(ExtractedJarError::NotAJar) => (),
                        Err(e) => return Err(e.into()),
                        Ok(extracted) => {
                            let mut largest: Option<JavaClass> = None;
                            for file in extracted.classfiles {
                                let p = file.to_string_lossy();
                                debug!("Reading from {p}");
                                let class = handle_class(file)?;
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
                                log!("Largest class version is {class}");
                                handle_too_high(&class, &max, &mut too_high);
                            } else {
                                warn!("Failed to find a single class");
                            }
                        }
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    if let (Some(max), Some(too_high)) = (max, too_high) {
        bail!("Found a class with version {too_high:?}, which is higher than the given maximum of {max:?}!");
    }

    Ok(())
}
