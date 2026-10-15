use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=FBFAST_BUILD_ID");
    for (key, source) in [("FBFAST_PROFILE", "PROFILE"), ("FBFAST_TARGET", "TARGET")] {
        println!(
            "cargo:rustc-env={key}={}",
            env::var(source).unwrap_or_default()
        );
    }
    println!(
        "cargo:rustc-env=FBFAST_BUILD_ID={}",
        env::var("FBFAST_BUILD_ID").unwrap_or_else(|_| "development".into())
    );
}
