use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct BuildInfo {
    pub version: &'static str,
    pub profile: &'static str,
    pub target: &'static str,
    pub build_id: &'static str,
}

pub fn build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        profile: env!("FBFAST_PROFILE"),
        target: env!("FBFAST_TARGET"),
        build_id: env!("FBFAST_BUILD_ID"),
    }
}
