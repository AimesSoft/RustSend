//! Platform operations use the mobile host and desktop native libraries.
use anyhow::Result;
use std::path::Path;

pub fn open_path(path: &Path) -> Result<()> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        bern::platform::open_path(path).map_err(|error| anyhow::Error::msg(error.message))
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        open::that_detached(path)?;
        Ok(())
    }
}

pub fn copy_text(text: &str) -> Result<()> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        bern::platform::copy_text(text).map_err(|error| anyhow::Error::msg(error.message))
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        arboard::Clipboard::new()?.set_text(text.to_owned())?;
        Ok(())
    }
}

pub fn export_path(source: &Path, directory: &str) -> Result<()> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        bern::platform::export_path(source, directory)
            .map(|_| ())
            .map_err(|error| anyhow::Error::msg(error.message))
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = (source, directory);
        anyhow::bail!("Use your file manager to copy received files on this platform")
    }
}
