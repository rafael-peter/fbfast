use std::path::Path;

pub fn client_info(path: &Path) -> Result<fbfast_native::ClientInfo, crate::error::Error> {
    Ok(fbfast_native::Client::load(path)?.info().clone())
}
