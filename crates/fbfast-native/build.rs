use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=FBFAST_CLIENT_ROOT");
    println!("cargo:rerun-if-changed=bridge");
    let root = env::var_os("FBFAST_CLIENT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Library/Frameworks/Firebird.framework"));
    let include = [root.join("include"), root.join("Headers")]
        .into_iter()
        .find(|p| p.join("firebird/Interface.h").is_file())
        .unwrap_or_else(|| panic!("Set FBFAST_CLIENT_ROOT to a Firebird 5 SDK (include/firebird/Interface.h or Headers/firebird/Interface.h)"));
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .include(include)
        .file("bridge/client.cpp")
        .file("bridge/signals.cpp")
        .file("bridge/status.cpp")
        .file("bridge/attachment.cpp")
        .file("bridge/transaction.cpp")
        .file("bridge/statement.cpp")
        .file("bridge/metadata.cpp")
        .file("bridge/cursor.cpp")
        .file("bridge/blob.cpp")
        .file("bridge/decimal.cpp")
        .file("bridge/zoned.cpp")
        .file("bridge/control.cpp")
        .warnings(true)
        .compile("fbfast_bridge");
}
