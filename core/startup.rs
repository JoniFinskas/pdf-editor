use crate::{Result, err};
use std::{ffi::OsString, path::PathBuf};

/// Parse arguments after the program name without losing Unicode/OS path data.
pub fn startup_path(args: impl IntoIterator<Item = OsString>) -> Result<Option<PathBuf>> {
    let mut args = args.into_iter();
    let first = args.next();
    let path = if first.as_deref() == Some(std::ffi::OsStr::new("--")) {
        args.next()
    } else {
        first
    };
    if args.next().is_some() {
        return Err(err("Usage: pdf-editor [PDF path]"));
    }
    match path {
        Some(path) if path.is_empty() => Err(err("PDF path cannot be empty")),
        Some(path) => Ok(Some(PathBuf::from(path))),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_preserves_spaced_unicode_paths_and_rejects_extra_arguments() {
        assert!(startup_path(Vec::new()).unwrap().is_none());
        let path = OsString::from("C:/Documents/My résumé ä.pdf");
        assert_eq!(
            startup_path([path.clone()]).unwrap(),
            Some(PathBuf::from(&path))
        );
        assert_eq!(
            startup_path([OsString::from("--"), path.clone()]).unwrap(),
            Some(PathBuf::from(&path))
        );
        assert!(startup_path([path.clone(), path]).is_err());
        assert!(startup_path([OsString::new()]).is_err());
    }
}
