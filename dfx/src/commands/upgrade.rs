use crate::lib::{
    env::VersionEnv,
    error::{DfxError, DfxResult},
};
use clap::{App, Arg, ArgMatches, SubCommand};
use libflate::gzip::Decoder;
use semver::Version;
use serde::{Deserialize, Deserializer};
use std::{collections::HashMap, env, fs, os::unix::fs::PermissionsExt};
use tar::Archive;

pub fn construct() -> App<'static, 'static> {
    SubCommand::with_name("upgrade")
        .about("Upgrade DFX.")
        .arg(
            Arg::with_name("current-version")
                .hidden(true)
                .long("current-version")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("release-root")
                .default_value("https://sdk.dfinity.org")
                .hidden(true)
                .long("release-root")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("verbose")
                .help("Verbose output.")
                .long("verbose"),
        )
}

fn parse_semver<'de, D>(version: &str) -> Result<Version, D::Error>
where
    D: Deserializer<'de>,
{
    semver::Version::parse(&version)
        .map_err(|e| serde::de::Error::custom(format!("invalid SemVer: {}", e)))
}

fn deserialize_tags<'de, D>(deserializer: D) -> Result<HashMap<String, Version>, D::Error>
where
    D: Deserializer<'de>,
{
    let tags: HashMap<String, String> = Deserialize::deserialize(deserializer)?;
    let mut result = HashMap::<String, Version>::new();

    for (tag, version) in tags.into_iter() {
        result.insert(tag, parse_semver::<D>(&version)?);
    }

    Ok(result)
}

fn deserialize_versions<'de, D>(deserializer: D) -> Result<Vec<Version>, D::Error>
where
    D: Deserializer<'de>,
{
    let versions: Vec<String> = Deserialize::deserialize(deserializer)?;
    let mut result = Vec::with_capacity(versions.len());

    for version in versions.iter() {
        result.push(parse_semver::<D>(version)?);
    }

    Ok(result)
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct Manifest {
    #[serde(deserialize_with = "deserialize_tags")]
    tags: HashMap<String, Version>,
    #[serde(deserialize_with = "deserialize_versions")]
    versions: Vec<Version>,
}

pub fn is_upgrade_necessary(latest_version: Option<Version>, current: Version) -> bool {
    match latest_version {
        Some(latest) => latest > current,
        None => true,
    }
}

pub fn get_latest_version(
    release_root: &str,
    timeout: Option<std::time::Duration>,
) -> DfxResult<Version> {
    let url = reqwest::Url::parse(release_root)
        .map_err(|e| DfxError::InvalidArgument(format!("invalid release root: {}", e)))?;
    let manifest_url = url
        .join("manifest.json")
        .map_err(|e| DfxError::InvalidArgument(format!("invalid manifest URL: {}", e)))?;
    println!("Fetching manifest {}", manifest_url);
    let client = match timeout {
        Some(timeout) => reqwest::Client::builder().timeout(timeout),
        None => reqwest::Client::builder(),
    };

    let client = client.build()?;
    let mut response = client.get(manifest_url).send().map_err(DfxError::Reqwest)?;
    let status_code = response.status();

    if !status_code.is_success() {
        return Err(DfxError::InvalidData(format!(
            "unable to fetch manifest: {}",
            status_code.canonical_reason().unwrap_or("unknown error"),
        )));
    }

    let manifest: Manifest = response
        .json()
        .map_err(|e| DfxError::InvalidData(format!("invalid manifest: {}", e)))?;
    manifest
        .tags
        .get("latest")
        .ok_or_else(|| DfxError::InvalidData("expected field 'latest' in 'tags'".to_string()))
        .map(|v| v.clone())
}

fn get_latest_release(release_root: &str, version: &Version, arch: &str) -> DfxResult<()> {
    let url = reqwest::Url::parse(&format!(
        "{0}/downloads/dfx/{1}/{2}/dfx-{1}.tar.gz",
        release_root, version, arch
    ))
    .map_err(|e| DfxError::InvalidArgument(format!("invalid release root: {}", e)))?;
    println!("Downloading {}", url);
    let mut response = reqwest::get(url).map_err(DfxError::Reqwest)?;
    let mut decoder = Decoder::new(&mut response)
        .map_err(|e| DfxError::InvalidData(format!("unable to gunzip file: {}", e)))?;
    let mut archive = Archive::new(&mut decoder);
    let current_exe_path = env::current_exe().map_err(DfxError::Io)?;
    let current_exe_dir = current_exe_path.parent().unwrap(); // This should not fail
    println!("Unpacking");
    archive.unpack(&current_exe_dir)?;
    println!("Setting permissions");
    let mut permissions = fs::metadata(&current_exe_path)?.permissions();
    permissions.set_mode(0o775); // FIXME Preserve existing permissions
    fs::set_permissions(&current_exe_path, permissions)?;
    println!("Done");
    Ok(())
}

pub fn exec<T>(env: &T, args: &ArgMatches<'_>) -> DfxResult
where
    T: VersionEnv,
{
    // Find OS architecture.
    let os_arch = match std::env::consts::OS {
        "linux" => "x86_64-linux",
        "macos" => "x86_64-darwin",
        _ => panic!("Not supported architecture"),
    };
    let current_version = if let Some(version) = args.value_of("current-version") {
        version
    } else {
        env.get_version()
    };
    let current_version = Version::parse(current_version)
        .map_err(|e| DfxError::InvalidData(format!("invalid version: {}", e)))?;
    println!("Current version: {}", current_version);
    let release_root = args.value_of("release-root").unwrap();
    let latest_version = get_latest_version(release_root, None)?;

    if latest_version > current_version {
        println!("New version available: {}", latest_version);
        // TODO(eftychis): Find architecture
        get_latest_release(release_root, &latest_version, os_arch)?;
    } else {
        println!("Already up to date");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
      "tags": {
        "latest": "0.4.1"
      },
      "versions": [
        "0.3.1",
        "0.4.0",
        "0.4.1"
      ]
}"#;

    #[test]
    fn test_parse_manifest() {
        let manifest: Manifest = serde_json::from_str(&MANIFEST).unwrap();
        let mut tags = HashMap::new();
        tags.insert(
            "latest".to_string(),
            semver::Version::parse("0.4.1").unwrap(),
        );
        let versions: Vec<Version> = vec!["0.3.1", "0.4.0", "0.4.1"]
            .into_iter()
            .map(|v| semver::Version::parse(v).unwrap())
            .collect();
        assert_eq!(manifest.versions, versions);
    }

    #[test]
    fn test_get_latest_version() {
        let _ = env_logger::try_init();
        let _m = mockito::mock("GET", "/manifest.json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(MANIFEST)
            .create();
        let latest_version = get_latest_version(&mockito::server_url(), None);
        assert_eq!(latest_version.unwrap(), Version::parse("0.4.1").unwrap());
        let _m = mockito::mock("GET", "/manifest.json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("Not a valid JSON object")
            .create();
        let latest_version = get_latest_version(&mockito::server_url(), None);
        assert!(latest_version.is_err());
    }
}