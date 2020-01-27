//! File       : configure.rs
//! License    : Apache 2.0 with LLVM Exception
//! Copyright  : 2020 DFINITY Stiftung
//! Maintainer : Enzo Haussecker <enzo@dfinity.org>
//! Stability  : Experimental

use crate::config::dfinity::ConfigDefaultsBootstrap;
use crate::lib::environment::Environment;
use crate::lib::error::{DfxError, DfxResult};
use clap::ArgMatches;
use std::default::Default;
use std::fs;
use std::io::{Error, ErrorKind};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use url::{ParseError, Url};

/// TODO (enzo): Documentation.
pub fn configure(
    env: &dyn Environment,
    args: &ArgMatches<'_>,
) -> DfxResult<ConfigDefaultsBootstrap> {
    let config = get_config(env);
    let ip = get_ip(&config, args)?;
    let port = get_port(&config, args)?;
    let providers = get_providers(&config, args)?;
    let root = get_root(&config, env, args)?;
    Ok(ConfigDefaultsBootstrap {
        ip: Some(ip),
        port: Some(port),
        providers: Some(providers),
        root: Some(root),
    })
}

/// TODO (enzo): Documentation.
fn get_config(env: &dyn Environment) -> ConfigDefaultsBootstrap {
    env.get_config().map_or(Default::default(), |config| {
        config
            .get_config()
            .get_defaults()
            .get_bootstrap()
            .to_owned()
    })
}

/// TODO (enzo): Documentation.
fn get_root(
    config: &ConfigDefaultsBootstrap,
    env: &dyn Environment,
    args: &ArgMatches<'_>,
) -> DfxResult<PathBuf> {
    args.value_of("root")
        .map(|root| parse_dir(root))
        .unwrap_or_else(|| {
            config
                .root
                .clone()
                .map_or(
                    env.get_cache()
                        .get_binary_command_path("js-user-library/dist/bootstrap"),
                    Ok,
                )
                .and_then(|root| {
                    parse_dir(
                        root.to_str()
                            .expect("File path without invalid unicode characters"),
                    )
                })
        })
        .map_err(|err| DfxError::InvalidArgument(format!("Invalid root directory: {:?}", err)))
}

/// TODO (enzo): Documentation.
fn get_ip(config: &ConfigDefaultsBootstrap, args: &ArgMatches<'_>) -> DfxResult<IpAddr> {
    args.value_of("ip")
        .map(|ip| ip.parse())
        .unwrap_or_else(|| {
            let default = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
            Ok(config.ip.unwrap_or(default))
        })
        .map_err(|err| DfxError::InvalidArgument(format!("Invalid IP address: {}", err)))
}

/// TODO (enzo): Documentation.
fn get_port(config: &ConfigDefaultsBootstrap, args: &ArgMatches<'_>) -> DfxResult<u16> {
    args.value_of("port")
        .map(|port| port.parse())
        .unwrap_or_else(|| {
            let default = 8081;
            Ok(config.port.unwrap_or(default))
        })
        .map_err(|err| DfxError::InvalidArgument(format!("Invalid port number: {}", err)))
}

/// TODO (enzo): Documentation.
fn get_providers(
    config: &ConfigDefaultsBootstrap,
    args: &ArgMatches<'_>,
) -> DfxResult<Vec<String>> {
    args.values_of("providers")
        .map(|providers| {
            providers
                .map(|provider| parse_url(provider))
                .collect::<Result<_, _>>()
        })
        .unwrap_or_else(|| {
            let default = vec!["http://127.0.0.1:8080/api".to_string()];
            config.providers.clone().map_or(Ok(default), |providers| {
                if providers.is_empty() {
                    Err(ParseError::EmptyHost)
                } else {
                    providers
                        .iter()
                        .map(|provider| parse_url(provider))
                        .collect()
                }
            })
        })
        .map_err(|err| DfxError::InvalidArgument(format!("Invalid provider URL: {}", err)))
}

fn parse_dir(dir: &str) -> DfxResult<PathBuf> {
    fs::metadata(dir)
        .map(|_| PathBuf::from(dir))
        .map_err(|_| DfxError::Io(Error::new(ErrorKind::NotFound, dir)))
}

fn parse_url(url: &str) -> Result<String, ParseError> {
    Url::parse(url).map(|_| String::from(url))
}