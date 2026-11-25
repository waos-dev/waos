//! A kind of file a program opens, as its author declares it: the file name extensions the system
//! knows such a file by, what the system calls one, and where there is one its MIME type, as
//! Waft's `[[package.metadata.waft.file_associations]]` declares an app's. The system then offers
//! the program for such a file, which reaches the program as an open request.
//!
//! The invariant: an association a system could match no file to, one without extensions, an
//! extension written with its dot or empty, or one without a name, is refused where it is read,
//! so none reaches a system.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A kind of file the program opens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAssociation {
    /// File name extensions, without the dot.
    pub extensions: Vec<String>,
    /// What the system calls a file of this type: the Finder's Kind, Explorer's Type.
    pub name: String,
    /// The type's MIME type, `application/x-shockwave-flash`, where it has one: Linux associates
    /// files by it, so there a type without one is not associated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl FileAssociation {
    /// Refuses an association no system could match a file to.
    pub fn check(&self) -> Result<(), FileAssociationError> {
        if self.name.trim().is_empty() {
            return Err(FileAssociationError::NoName);
        }
        if self.extensions.is_empty() {
            return Err(FileAssociationError::NoExtensions(self.name.clone()));
        }
        let unusable = self
            .extensions
            .iter()
            .find(|extension| extension.is_empty() || extension.starts_with('.'));
        if let Some(extension) = unusable {
            return Err(FileAssociationError::NotAnExtension {
                name: self.name.clone(),
                extension: extension.clone(),
            });
        }
        Ok(())
    }
}

/// Why an association is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileAssociationError {
    /// Its name is empty, or nothing but whitespace.
    NoName,
    /// It names no extension, by its name.
    NoExtensions(String),
    /// An extension is empty or written with its dot.
    NotAnExtension { name: String, extension: String },
}

impl fmt::Display for FileAssociationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileAssociationError::NoName => f.write_str(
                "a file association has no name; it is what the system calls such a file",
            ),
            FileAssociationError::NoExtensions(name) => {
                write!(f, "the file association `{name}` has no extensions")
            }
            FileAssociationError::NotAnExtension { name, extension } => write!(
                f,
                "the file association `{name}`: `{extension}` is not an extension; write it without the dot"
            ),
        }
    }
}

impl std::error::Error for FileAssociationError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn flash() -> FileAssociation {
        FileAssociation {
            extensions: vec!["swf".to_owned()],
            name: "Flash movie".to_owned(),
            mime_type: Some("application/x-shockwave-flash".to_owned()),
        }
    }

    #[test]
    fn an_association_with_extensions_and_a_name_is_taken() {
        assert_eq!(flash().check(), Ok(()));
    }

    #[test]
    fn an_association_no_system_could_match_a_file_to_is_refused() {
        let dotted = FileAssociation {
            extensions: vec![".swf".to_owned()],
            ..flash()
        };
        assert!(matches!(
            dotted.check(),
            Err(FileAssociationError::NotAnExtension { .. })
        ));
        let none = FileAssociation {
            extensions: Vec::new(),
            ..flash()
        };
        assert!(matches!(
            none.check(),
            Err(FileAssociationError::NoExtensions(_))
        ));
        let unnamed = FileAssociation {
            name: " ".to_owned(),
            ..flash()
        };
        assert_eq!(unnamed.check(), Err(FileAssociationError::NoName));
    }
}
