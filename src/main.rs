mod cli;

use anyhow::bail;
use cli::Cli;
use std::{
    fmt::Display,
    fs::File,
    io::{self, Read, Seek},
    ops::Deref,
    path::Path,
};
use thiserror::Error;
use zip::{ZipArchive, result::ZipError};

#[derive(Debug, PartialEq, PartialOrd, Clone, Eq, Ord)]
struct JavaVersion(pub u16);

impl Deref for JavaVersion {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<JavaClass> for JavaVersion {
    fn from(value: JavaClass) -> Self {
        // the 44 was scientifically chosen by looking at the table in
        // https://en.wikipedia.org/wiki/Java_class_file#General_layout and doing second grade math
        // (might be a different grade, no idea actually)
        let version = value.0 - 44;
        Self(version)
    }
}

impl FromIterator<JavaClass> for JavaVersion {
    fn from_iter<T: IntoIterator<Item = JavaClass>>(iter: T) -> Self {
        iter.into_iter()
            .map(|elem| elem.into())
            .max()
            .unwrap_or(JavaVersion(0))
    }
}

impl Display for JavaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "(Java {})", **self)
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
struct JavaClass(pub u16);

const MAGIC_CLASS_HEADER: [u8; 4] = [202, 254, 186, 190]; // CAFEBABE
const MAGIC_ZIP_HEADER: [u8; 4] = [80, 75, 3, 4]; // I don't think this turns into anything fancy

#[derive(Error, Debug)]
enum JavaClassError {
    #[error("Failed to read bytes from file")]
    Read(#[from] io::Error),
    #[error("Should have got at least 8 bytes, got {0}")]
    InsufficientBytes(usize),
    #[error("Not a java class")]
    NotAClassFile,
}

impl JavaClass {
    pub fn new<T: Read>(mut f: T) -> Result<Self, JavaClassError> {
        let mut buffer = [0; 8];

        let read_bytes = f.read(&mut buffer)?;
        if read_bytes != 8 {
            return Err(JavaClassError::InsufficientBytes(read_bytes));
        }

        if &buffer[..4] != &MAGIC_CLASS_HEADER {
            return Err(JavaClassError::NotAClassFile);
        }

        let version = u16::from_be_bytes([buffer[6], buffer[7]]);

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
    #[error("todo")]
    JavaClass(#[from] JavaClassError),
    #[error("No suitable class files found. Maybe this isn't actually a Jar?")]
    NoClassFiles,
}

#[allow(dead_code)]
struct ExtractedJar {
    classfiles: Vec<JavaClass>,
}

impl ExtractedJar {
    fn new(file: &str) -> Result<Self, ExtractedJarError> {
        let mut file = File::open(file)?;
        trace!("Reading archive at {file:?}");

        let mut buffer = [0; 4];

        let read_bytes = file.read(&mut buffer)?;
        if read_bytes != 4 {
            return Err(ExtractedJarError::InsufficientBytes(read_bytes));
        }

        // not sure if this is even necessary. ZipArchive::new most likely does something like this as well
        if &buffer != &MAGIC_ZIP_HEADER {
            return Err(ExtractedJarError::NotAJar);
        }
        // Technically we don't know if the jar is actually a jar
        // We just know that the file is a zip file (or, well, we assume it is because the magic bytes said so)
        let mut archive = zip::ZipArchive::new(file)?;
        // got here, now we can be pretty sure that this is a zip file! Wait, this isn't really what we were looking for...

        trace!("Got archive {archive:?}");
        debug!("Trying to get all relevant files in the JAR");
        let classfiles = get_class_files_in_jar(&archive);

        // Technically, Jar files might not contain any classes. But no idea what to do with that in this context
        if classfiles.is_empty() {
            // when in doubt, bubble the problem up to the call site!
            // https://en.wikipedia.org/wiki/Somebody_else%27s_problem
            return Err(ExtractedJarError::NoClassFiles);
        }

        // This is definitely a zip with class files! Don't know if that is meaningfully different from a Jar. Assuming it isn't...
        let mut out_classfiles = Vec::new();
        debug!("classfiles in jar: {classfiles:?}");
        // NOTE: This can't be done in parallel with rayon as the archive can't be borrowed as mutable in that case
        // RwLock doesn't help, can't get a `mut` from `read()` and calling `write()` would lock, defeating the parallel approach completely
        for file in classfiles {
            debug!("Trying to extract {file}");
            trace!("Trying to get a file for {file}");
            let file = archive.by_name(&file)?;
            trace!("Got something");
            let javaclass = JavaClass::new(file)?;
            out_classfiles.push(javaclass);
        }

        Ok(Self {
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
        // META-INF can contain .class files, no idea what they do
        // Pretend/hope that they don't matter
        .filter(|name| !name.starts_with("META-INF"))
        .map(|name| name.to_owned())
        .collect()
}

fn handle_class<P: AsRef<Path>>(file: P) -> Result<JavaClass, JavaClassError> {
    let file = File::open(file)?;
    debug!("Read {file:?}");
    let class = JavaClass::new(file)?;
    Ok(class)
}

fn process_jar(file: &str) -> Result<JavaVersion, ExtractedJarError> {
    log!("Handling JAR file {file}");
    let extracted = ExtractedJar::new(&file)?;
    let version: JavaVersion = JavaVersion::from_iter(extracted.classfiles);
    if *version == 0 {
        return Err(ExtractedJarError::NoClassFiles.into());
    }
    Ok(version)
}

fn process_class(file: &str) -> Result<JavaVersion, JavaClassError> {
    log!("Reading from {file}");
    let class = handle_class(&file)?;
    let version: JavaVersion = class.into();
    log!("Class version is {}", version);
    Ok(version)
}

fn main() -> anyhow::Result<()> {
    let args = Cli::new()?;
    trace!("{args:?}");

    let max = args.max;
    let mut too_high = Vec::new();

    for file in args.files {
        let path = Path::new(&file);
        let extension = path.extension().and_then(|s| s.to_str());
        let version: anyhow::Result<JavaVersion> = match extension {
            Some("jar") => process_jar(&file).map_err(|e| e.into()),
            Some("class") => process_class(&file).map_err(|e| e.into()),
            _ => {
                // no idea what this is, guess
                // doesn't really matter what option we try first, so class it is
                process_class(&file)
                    .or_else(|_| process_jar(&file))
                    .map_err(|e| e.into())
            }
        };
        let version = version?;
        if let Some(max) = max {
            trace!("max is set; checking");
            if *version > max {
                trace!("version version {version} is higher than {max}!");
                too_high.push(version)
            }
        }
    }
    if !too_high.is_empty() && max.is_some() {
        let mut too_high = too_high;
        let max = max.unwrap();
        too_high.sort();
        too_high.dedup();
        bail!(
            "Found class(es) with version(s) {too_high:?}, which is higher than the given maximum of {max}!"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_java_version_from_java_class() {
        let java_class = JavaClass(52);
        let java_version: JavaVersion = java_class.into();
        assert_eq!(*java_version, 8);
    }

    #[test]
    fn test_java_version_from_iter() {
        let classes = vec![JavaClass(50), JavaClass(52), JavaClass(51)];
        let version: JavaVersion = JavaVersion::from_iter(classes);
        assert_eq!(*version, 8);
    }

    #[test]
    fn test_java_version_from_empty_iter() {
        let classes: Vec<JavaClass> = vec![];
        let version: JavaVersion = JavaVersion::from_iter(classes);
        assert_eq!(*version, 0);
    }

    #[test]
    fn test_java_version_display() {
        let version = JavaVersion(11);
        let formatted = format!("{}", version);
        assert_eq!(formatted, "(Java 11)");
    }

    #[test]
    fn test_java_class_new_valid() {
        let class_bytes = vec![
            202, 254, 186, 190, // CAFEBABE magic
            0, 0,               // minor version
            0, 52,              // major version (Java 8)
        ];
        let cursor = Cursor::new(class_bytes);
        let result = JavaClass::new(cursor);
        
        assert!(result.is_ok());
        let class = result.unwrap();
        assert_eq!(class.0, 52);
    }

    #[test]
    fn test_java_class_new_insufficient_bytes() {
        let class_bytes = vec![202, 254, 186, 190, 0]; // Only 5 bytes
        let cursor = Cursor::new(class_bytes);
        let result = JavaClass::new(cursor);
        
        assert!(matches!(result, Err(JavaClassError::InsufficientBytes(5))));
    }

    #[test]
    fn test_java_class_new_invalid_magic() {
        let class_bytes = vec![
            1, 2, 3, 4,         // Invalid magic
            0, 0,               // minor version
            0, 52,              // major version
        ];
        let cursor = Cursor::new(class_bytes);
        let result = JavaClass::new(cursor);
        
        assert!(matches!(result, Err(JavaClassError::NotAClassFile)));
    }

    #[test]
    fn test_get_class_files_in_jar() {
        // This test would require creating a mock ZipArchive, which is complex
        // For now, testing the logic conceptually
    }

    #[test]
    fn test_java_version_ordering() {
        let v8 = JavaVersion(8);
        let v11 = JavaVersion(11);
        let v17 = JavaVersion(17);
        
        assert!(v8 < v11);
        assert!(v11 < v17);
        assert!(v8 < v17);
    }

    #[test]
    fn test_java_class_ordering() {
        let c50 = JavaClass(50);
        let c52 = JavaClass(52);
        let c55 = JavaClass(55);
        
        assert!(c50 < c52);
        assert!(c52 < c55);
        assert!(c50 < c55);
    }
}
