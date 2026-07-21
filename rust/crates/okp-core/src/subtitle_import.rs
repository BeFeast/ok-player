use std::path::{Path, PathBuf};

/// Where a local external-subtitle file came from before it entered the player.
/// The shell deliberately treats every origin identically once the file exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExternalSubtitleOrigin {
    LocalFile,
    OnlineSearch { provider: String, result_id: String },
}

/// A local file ready for the existing external-subtitle load pipeline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalSubtitleImport {
    path: PathBuf,
    origin: ExternalSubtitleOrigin,
}

impl ExternalSubtitleImport {
    #[must_use]
    pub fn local(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            origin: ExternalSubtitleOrigin::LocalFile,
        }
    }

    pub(crate) fn online_search(
        path: impl Into<PathBuf>,
        provider: impl Into<String>,
        result_id: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            origin: ExternalSubtitleOrigin::OnlineSearch {
                provider: provider.into(),
                result_id: result_id.into(),
            },
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn origin(&self) -> &ExternalSubtitleOrigin {
        &self.origin
    }

    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_sidecar_uses_the_shared_external_import_model() {
        let import = ExternalSubtitleImport::local("Movie.en.srt");

        assert_eq!(import.path(), Path::new("Movie.en.srt"));
        assert_eq!(import.origin(), &ExternalSubtitleOrigin::LocalFile);
        assert_eq!(import.into_path(), PathBuf::from("Movie.en.srt"));
    }
}
