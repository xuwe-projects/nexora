use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use updater::{LoadedApplicationReleaseMetadata, ReleaseMetadataError};

const UPDATER_HEALTH_ARGUMENTS: [&str; 2] = [
    "--nexora-updater-health-session",
    "--nexora-updater-health-file",
];

pub(crate) fn resolve_with_release_loader(
    explicit_path: Option<PathBuf>,
    arguments: impl IntoIterator<Item = OsString>,
    application_name: &str,
    manifest_directory: &str,
    load_release: impl FnOnce()
        -> Result<Option<LoadedApplicationReleaseMetadata>, ReleaseMetadataError>,
) -> Result<PathBuf, ReleaseMetadataError> {
    if let Some(path) = explicit_path {
        return Ok(path);
    }
    if let Some(path) = from_args(arguments) {
        return Ok(path);
    }
    if let Some(release) = load_release()? {
        return Ok(release
            .resource_directory()
            .join("config")
            .join(format!("{application_name}.toml")));
    }
    Ok(development_path(application_name, manifest_directory))
}

pub(crate) fn from_args(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut arguments = args.into_iter().skip(1);
    while let Some(argument) = arguments.next() {
        if UPDATER_HEALTH_ARGUMENTS
            .iter()
            .any(|internal| argument == *internal)
        {
            _ = arguments.next();
            continue;
        }
        return Some(PathBuf::from(argument));
    }
    None
}

fn development_path(application_name: &str, manifest_directory: &str) -> PathBuf {
    let relative = PathBuf::from("config").join(format!("{application_name}.toml"));
    if relative.is_file() {
        return relative;
    }
    Path::new(manifest_directory)
        .ancestors()
        .map(|directory| directory.join(&relative))
        .find(|candidate| candidate.is_file())
        .unwrap_or(relative)
}
