use fbfast_core::{
    config::{Config, EnvFile, Inputs},
    execution::{
        control::{Cancellation, QueryOptions},
        ExecutionConnection,
    },
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
};

pub fn config() -> Config {
    let input: Inputs = [
        ("host".into(), json!("127.0.0.1")),
        (
            "port".into(),
            json!(env::var("FBFAST_TEST_PORT")
                .unwrap()
                .parse::<u16>()
                .unwrap()),
        ),
        (
            "database".into(),
            json!("/var/lib/firebird/data/fbfast.fdb"),
        ),
        ("user".into(), json!("FBFAST_READER")),
        (
            "password".into(),
            json!(env::var("FBFAST_TEST_PASSWORD").unwrap()),
        ),
        ("role".into(), json!("FBFAST_READONLY")),
    ]
    .into();
    Config::resolve_sources(
        input,
        "FB_",
        EnvFile::Disabled,
        &BTreeMap::new(),
        Path::new("."),
    )
    .unwrap()
}

pub fn client() -> PathBuf {
    env::var("FBFAST_TEST_CLIENT").unwrap().into()
}
pub fn connection() -> ExecutionConnection {
    ExecutionConnection::connect(config(), &client()).unwrap()
}
pub fn options(cancellation: &Cancellation) -> QueryOptions<'_> {
    QueryOptions {
        first: false,
        unique_names: false,
        deadline: None,
        cancellation,
        cleaning: &|| Ok(false),
    }
}
